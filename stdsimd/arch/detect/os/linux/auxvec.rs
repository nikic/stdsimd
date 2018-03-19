//! Parses ELF auxiliary vectors.

use core::mem;
use _std::prelude::v1::*;
use _std::fs::File;
use _std::io::Read;

/// Key to access the CPU Hardware capabilities bitfield.
pub const AT_HWCAP: usize = 16;
/// Key to access the CPU Hardware capabilities 2 bitfield.
#[cfg(any(target_arch = "arm", target_arch = "powerpc64"))]
pub const AT_HWCAP2: usize = 26;

/// Cache HWCAP bitfields of the ELF Auxiliary Vector.
///
/// If an entry cannot be read all the bits in the bitfield are set to zero.
/// This should be interpreted as all the features being disabled.
#[derive(Debug, Copy, Clone)]
pub struct AuxVec {
    pub hwcap: usize,
    #[cfg(any(target_arch = "arm", target_arch = "powerpc64"))]
    pub hwcap2: usize,
}

/// ELF Auxiliary Vector
///
/// The auxiliary vector is a memory region in a running ELF program's stack
/// composed of (key: usize, value: usize) pairs.
///
/// The keys used in the aux vector are platform dependent. For Linux, they are
/// defined in [linux/auxvec.h][auxvec_h]. The hardware capabilities of a given
/// CPU can be queried with the  `AT_HWCAP` and `AT_HWCAP2` keys.
///
/// There is no perfect way of reading the auxiliary vector.
///
/// - If the `getauxval` is dynamically linked to this binary, it will be used.
/// - Otherwise, try to read `/proc/self/auxv`.
/// - If that fails, this function returns an error.
///
/// Note that run-time feature detection is not invoked for features that can
/// be detected at compile-time. Also note that if this function returns an
/// error, cpuinfo still can (and will) be used to try to perform run-time
/// feature detecton on some platforms.
///
/// For more information about when `getauxval` is available check the great
/// [`auxv` crate documentation][auxv_docs].
///
/// [auxvec_h]: https://github.com/torvalds/linux/blob/master/include/uapi/linux/auxvec.h
/// [auxv_docs]: https://docs.rs/auxv/0.3.3/auxv/
pub fn auxv() -> Result<AuxVec, ()> {
    // Try to call a dynamically-linked getauxval function.
    if let Ok(hwcap) = getauxval(AT_HWCAP) {
        // Targets with only AT_HWCAP:
        #[cfg(any(target_arch = "aarch64", target_arch = "mips",
                  target_arch = "mips64"))]
        {
            if hwcap != 0 {
                return Ok(AuxVec { hwcap });
            }
        }

        // Targets with AT_HWCAP and AT_HWCAP2:
        #[cfg(any(target_arch = "arm", target_arch = "powerpc64"))]
        {
            if let Ok(hwcap2) = getauxval(AT_HWCAP2) {
                if hwcap != 0 && hwcap2 != 0 {
                    return Ok(AuxVec { hwcap, hwcap2 });
                }
            }
        }
    }
    // If calling getauxval fails, try to read the auxiliary vector from
    // its file:
    auxv_from_file("/proc/self/auxv")
}

/// Tries to read the `key` from the auxiliary vector by calling the
/// dynamically-linked `getauxval` function. If the function is not linked,
/// this function return `Err`.
fn getauxval(key: usize) -> Result<usize, ()> {
    use libc;
    pub type F = unsafe extern "C" fn(usize) -> usize;
    unsafe {
        let ptr = libc::dlsym(
            libc::RTLD_DEFAULT,
            "getauxval\0".as_ptr() as *const _,
        );
        if ptr.is_null() {
            return Err(());
        }

        let ffi_getauxval: F = mem::transmute(ptr);
        Ok(ffi_getauxval(key))
    }
}

/// Tries to read the auxiliary vector from the `file`. If this fails, this
/// function returns `Err`.
fn auxv_from_file(file: &str) -> Result<AuxVec, ()> {
    let mut file = File::open(file).map_err(|_| ())?;

    // See https://github.com/torvalds/linux/blob/v3.19/include/uapi/linux/auxvec.h
    //
    // The auxiliary vector contains at most 32 (key,value) fields: from
    // `AT_EXECFN = 31` to `AT_NULL = 0`. That is, a buffer of
    // 2*32 `usize` elements is enough to read the whole vector.
    let mut buf = [0_usize; 64];
    {
        let raw: &mut [u8; 64 * mem::size_of::<usize>()] =
            unsafe { mem::transmute(&mut buf) };
        file.read(raw).map_err(|_| ())?;
    }
    auxv_from_buf(&buf)
}

/// Tries to interpret the `buffer` as an auxiliary vector. If that fails, this
/// function returns `Err`.
fn auxv_from_buf(buf: &[usize; 64]) -> Result<AuxVec, ()> {
    // Targets with only AT_HWCAP:
    #[cfg(any(target_arch = "aarch64", target_arch = "mips",
              target_arch = "mips64"))]
    {
        for el in buf.chunks(2) {
            match el[0] {
                AT_HWCAP => return Ok(AuxVec { hwcap: el[1] }),
                _ => (),
            }
        }
    }
    // Targets with AT_HWCAP and AT_HWCAP2:
    #[cfg(any(target_arch = "arm", target_arch = "powerpc64"))]
    {
        let mut hwcap = None;
        let mut hwcap2 = None;
        for el in buf.chunks(2) {
            match el[0] {
                AT_HWCAP => hwcap = Some(el[1]),
                AT_HWCAP2 => hwcap2 = Some(el[1]),
                _ => (),
            }
        }

        if let (Some(hwcap), Some(hwcap2)) = (hwcap, hwcap2) {
            return Ok(AuxVec { hwcap, hwcap2 });
        }
    }
    Err(())
}

#[cfg(test)]
mod tests {
    extern crate auxv as auxv_crate;
    use super::*;

    // Reads the Auxiliary Vector key from /proc/self/auxv
    // using the auxv crate.
    fn auxv_crate_getprocfs(key: usize) -> Option<usize> {
        use self::auxv_crate::AuxvType;
        use self::auxv_crate::procfs::search_procfs_auxv;
        let k = key as AuxvType;
        match search_procfs_auxv(&[k]) {
            Ok(v) => Some(v[&k] as usize),
            Err(_) => None,
        }
    }

    // Reads the Auxiliary Vector key from getauxval()
    // using the auxv crate.
    #[cfg(not(any(target_arch = "mips", target_arch = "mips64")))]
    fn auxv_crate_getauxval(key: usize) -> Option<usize> {
        use self::auxv_crate::AuxvType;
        use self::auxv_crate::getauxval::Getauxval;
        let q = auxv_crate::getauxval::NativeGetauxval {};
        match q.getauxval(key as AuxvType) {
            Ok(v) => Some(v as usize),
            Err(_) => None,
        }
    }

    // FIXME: on mips/mips64 getauxval returns 0, and /proc/self/auxv
    // does not always contain the AT_HWCAP key under qemu.
    #[cfg(not(any(target_arch = "mips", target_arch = "mips64")))]
    #[test]
    fn auxv_crate() {
        let v = auxv();
        if let Some(hwcap) = auxv_crate_getauxval(AT_HWCAP) {
            let rt_hwcap = v.expect("failed to find hwcap key").hwcap;
            assert_eq!(rt_hwcap, hwcap);
        }

        // Targets with AT_HWCAP and AT_HWCAP2:
        #[cfg(any(target_arch = "arm", target_arch = "powerpc64"))]
        {
            if let Some(hwcap2) = auxv_crate_getauxval(AT_HWCAP2) {
                let rt_hwcap2 = v.expect("failed to find hwcap2 key").hwcap2;
                assert_eq!(rt_hwcap2, hwcap2);
            }
        }
    }

    #[test]
    fn auxv_dump() {
        if let Ok(auxvec) = auxv() {
            println!("{:?}", auxvec);
        } else {
            println!("both getauxval() and reading /proc/self/auxv failed!");
        }
    }

    cfg_if! {
        if #[cfg(target_arch = "arm")] {
            #[test]
            fn linux_rpi3() {
                let v = auxv_from_file(
                    "../../stdsimd/arch/detect/test_data/linux-rpi3.auxv",
                ).unwrap();
                assert_eq!(v.hwcap, 4174038);
                assert_eq!(v.hwcap2, 16);
            }

            #[test]
            #[should_panic]
            fn linux_macos_vb() {
                let _ = auxv_from_file(
                    "../../stdsimd/arch/detect/test_data/macos-virtualbox-linux-x86-4850HQ.auxv"
                ).unwrap();
                // this file is incomplete (contains hwcap but not hwcap2), we
                // want to fall back to /proc/cpuinfo in this case, so
                // reading should fail. assert_eq!(v.hwcap, 126614527);
                // assert_eq!(v.hwcap2, 0);
            }
        } else if #[cfg(target_arch = "aarch64")] {
            #[test]
            fn linux_x64() {
                let v = auxv_from_file(
                    "../../stdsimd/arch/detect/test_data/linux-x64-i7-6850k.auxv",
                ).unwrap();
                assert_eq!(v.hwcap, 3219913727);
            }
        }
    }

    #[test]
    fn auxv_dump_procfs() {
        if let Ok(auxvec) = auxv_from_file("/proc/self/auxv") {
            println!("{:?}", auxvec);
        } else {
            println!("reading /proc/self/auxv failed!");
        }
    }

    #[test]
    fn auxv_crate_procfs() {
        let v = auxv();
        if let Some(hwcap) = auxv_crate_getprocfs(AT_HWCAP) {
            assert_eq!(v.unwrap().hwcap, hwcap);
        }

        // Targets with AT_HWCAP and AT_HWCAP2:
        #[cfg(any(target_arch = "arm", target_arch = "powerpc64"))]
        {
            if let Some(hwcap2) = auxv_crate_getprocfs(AT_HWCAP2) {
                assert_eq!(v.unwrap().hwcap2, hwcap2);
            }
        }
    }
}