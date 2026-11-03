use crate::{Result, invalid};

#[cfg(target_arch = "x86_64")]
mod normalizer;

pub(super) struct Admission {
    #[cfg(target_arch = "x86_64")]
    branch: normalizer::Branch,
}

impl Admission {
    pub(super) fn modern() -> Result<Self> {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx")
            && std::arch::is_x86_feature_detected!("fma")
            && std::arch::is_x86_feature_detected!("avx2")
        {
            use std::arch::x86_64::_mm_getcsr;
            #[allow(deprecated)]
            if unsafe { _mm_getcsr() } & !0x3f != 0x1f80 {
                return Err(invalid("attention floating-point controls are unsupported"));
            }
            return Ok(Self {
                branch: normalizer::Branch::AvxFma,
            });
        }
        Err(invalid("unsupported CPU features for attention"))
    }

    pub(super) fn check_width(width: usize) -> Result<()> {
        #[cfg(target_arch = "x86_64")]
        if normalizer::supported_width(width) {
            return Ok(());
        }
        Err(invalid("unsupported attention normalization width"))
    }

    pub(super) fn scores(
        &self,
        input_width: usize,
        key_rows: usize,
        scores: &mut [f32],
    ) -> Result<()> {
        Self::check_width(key_rows)?;
        if input_width == 0
            || input_width > i32::MAX as usize
            || scores.len() < key_rows.div_ceil(16) * 16
        {
            return Err(invalid("attention score dimensions"));
        }
        #[cfg(target_arch = "x86_64")]
        {
            use std::arch::x86_64::*;
            #[allow(deprecated)]
            if unsafe { _mm_getcsr() } & !0x3f != 0x1f80 {
                return Err(invalid("attention floating-point controls are unsupported"));
            }
            unsafe { scale_and_unmasked(input_width, key_rows, scores) };
            if !unsafe {
                normalizer::normalize_raw(self.branch, key_rows, scores.as_mut_ptr(), scores.len())
            } {
                return Err(invalid("attention normalization is unavailable"));
            }
            if scores.iter().all(|v| v.is_finite()) {
                return Ok(());
            }
            return Err(invalid("nonfinite attention probabilities"));
        }
        #[cfg(not(target_arch = "x86_64"))]
        Err(invalid("unsupported CPU features for attention"))
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx,fma")]
unsafe fn scale_and_unmasked(input_width: usize, key_rows: usize, scores: &mut [f32]) {
    use std::arch::x86_64::*;
    let scale = _mm_div_ss(_mm_set_ss(1.0), _mm_sqrt_ss(_mm_set_ss(input_width as f32)));
    for value in &mut scores[..key_rows.div_ceil(8) * 8] {
        let scaled = _mm_mul_ss(_mm_set_ss(*value), scale);
        *value = _mm_cvtss_f32(_mm_fmadd_ss(
            _mm_setzero_ps(),
            _mm_set_ss(f32::from_bits(0xd01502f9)),
            scaled,
        ));
    }
}
