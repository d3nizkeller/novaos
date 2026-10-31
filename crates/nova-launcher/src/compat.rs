//! Совместимость со старой glibc телевизионных сборок.
//!
//! webOS 3–4 поставляется с glibc 2.12, в которой ещё нет `getauxval`
//! (появилась в 2.16). Свежий `std` и `ring` ожидают этот символ на ARM,
//! поэтому для `arm-unknown-linux-gnueabi` мы приносим свою реализацию:
//! она читает вспомогательный вектор процесса из `/proc/self/auxv`.

use std::ffi::c_ulong;

/// Читает auxiliary vector процесса (пары «тип, значение»).
fn aux_value(kind: c_ulong) -> c_ulong {
    let Ok(blob) = std::fs::read("/proc/self/auxv") else {
        return 0;
    };
    let width = std::mem::size_of::<c_ulong>();
    for chunk in blob.chunks_exact(width * 2) {
        let key = read_uint(&chunk[..width]);
        let value = read_uint(&chunk[width..]);
        if key == kind {
            return value;
        }
        if key == 0 {
            break;
        }
    }
    0
}

/// Читает машинное целое из среза auxv.
fn read_uint(bytes: &[u8]) -> c_ulong {
    let mut buffer = [0u8; std::mem::size_of::<c_ulong>()];
    buffer.copy_from_slice(bytes);
    c_ulong::from_ne_bytes(buffer)
}

/// Замена `getauxval` из glibc ≥ 2.16.
///
/// # Safety
/// Сигнатура совпадает с libc: единственный аргумент — тип поля
/// вспомогательного вектора (AT_HWCAP = 16, AT_HWCAP2 = 26).
#[no_mangle]
pub extern "C" fn getauxval(kind: c_ulong) -> c_ulong {
    aux_value(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_aux_type_is_zero() {
        // Тип 0x7fff_ffff в auxv не встречается — ожиданием будет 0.
        assert_eq!(aux_value(0x7fff_ffff), 0);
    }

    #[test]
    fn hwcap_is_readable_on_linux() {
        // На Linux /proc/self/auxv существует, поэтому AT_HWCAP (16)
        // возвращает ненулевое значение; на других ОС допустим 0.
        let value = aux_value(16);
        if cfg!(target_os = "linux") {
            assert_ne!(value, 0, "auxv должен читаться");
        } else {
            let _ = value;
        }
    }
}
