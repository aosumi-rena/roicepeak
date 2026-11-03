#[derive(Clone, Copy)]
pub(super) struct Mode {
    pub flag: u8,
    pub fused: bool,
    pub alternate: bool,
}

impl Mode {
    pub fn from_flag(flag: u8) -> Option<Self> {
        matches!(flag, 0 | 1 | 3).then_some(Self {
            flag,
            fused: flag != 0,
            alternate: flag == 3,
        })
    }

    pub fn current() -> Self {
        let fused = super::postfilter_log::fused_available();
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let alternate = fused && std::is_x86_feature_detected!("avx2");
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let alternate = false;
        Self {
            flag: if alternate { 3 } else { u8::from(fused) },
            fused,
            alternate,
        }
    }
}
