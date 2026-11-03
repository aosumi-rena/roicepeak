#[path = "matrix_storage/fused.rs"]
mod fused;
#[path = "matrix_storage/input_quantization.rs"]
mod input_quantization;
#[path = "matrix_storage/quantized.rs"]
mod quantized;
#[cfg(target_arch = "x86_64")]
#[path = "matrix_storage/quantized8.rs"]
mod quantized8;

use crate::{Result, dnni::Node, invalid, tensor::Tensor};
use quantized::QuantizedMatrix;
pub(super) use quantized::{Accumulate, Workspace};
use std::sync::OnceLock;

#[derive(Clone, Copy)]
struct Context {
    integer16: bool,
    integer8: bool,
    fused: bool,
}

impl Context {
    fn detected() -> &'static Self {
        static CONTEXT: OnceLock<Context> = OnceLock::new();
        CONTEXT.get_or_init(|| {
            #[cfg(target_arch = "x86_64")]
            {
                Self {
                    integer16: std::arch::is_x86_feature_detected!("ssse3"),
                    integer8: std::arch::is_x86_feature_detected!("ssse3")
                        && std::arch::is_x86_feature_detected!("sse4.1"),
                    fused: std::arch::is_x86_feature_detected!("avx")
                        && std::arch::is_x86_feature_detected!("fma"),
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                Self {
                    integer16: false,
                    integer8: false,
                    fused: false,
                }
            }
        })
    }
}

#[derive(Debug)]
pub(crate) enum Matrix {
    Dense(Tensor),
    Fused(fused::FusedMatrix),
    Quantized(QuantizedMatrix),
    #[cfg(target_arch = "x86_64")]
    Quantized8(quantized8::QuantizedMatrix),
}

impl Matrix {
    pub(super) fn from_node(node: &Node<'_>) -> Result<Self> {
        let matrix = match node.tag.as_str() {
            "prim0" => Ok(Self::Dense(node.tensor()?)),
            "prim4" => match crate::tensor::u32_at(node.payload, 0)? {
                16 if Context::detected().integer16 => {
                    Ok(Self::Quantized(QuantizedMatrix::from_node(node)?))
                }
                #[cfg(target_arch = "x86_64")]
                8 if Context::detected().integer8 => Ok(Self::Quantized8(
                    quantized8::QuantizedMatrix::from_node(node)?,
                )),
                8 | 16 => Err(invalid("quantized matrix CPU features are unavailable")),
                _ => Err(invalid("unsupported quantized matrix bit width")),
            },
            _ => Err(invalid("expected affine/convolution matrix primitive")),
        }?;
        if matrix.is_quantized() && (matrix.rows() == 0 || matrix.cols() == 0) {
            return Err(invalid(
                "retained quantized matrix requires positive dimensions",
            ));
        }
        Ok(matrix)
    }

    pub(crate) fn rows(&self) -> usize {
        match self {
            Self::Dense(m) => m.rows(),
            Self::Fused(m) => m.rows(),
            Self::Quantized(m) => m.rows(),
            #[cfg(target_arch = "x86_64")]
            Self::Quantized8(m) => m.rows(),
        }
    }
    pub(crate) fn cols(&self) -> usize {
        match self {
            Self::Dense(m) => m.cols(),
            Self::Fused(m) => m.cols(),
            Self::Quantized(m) => m.cols(),
            #[cfg(target_arch = "x86_64")]
            Self::Quantized8(m) => m.cols(),
        }
    }
    pub(super) fn elements(&self) -> usize {
        match self {
            Self::Fused(m) => m.work_elements(),
            _ => self.rows().saturating_mul(self.cols()),
        }
    }
    pub(super) fn is_quantized(&self) -> bool {
        !matches!(self, Self::Dense(_) | Self::Fused(_))
    }
    pub(super) fn into_product_kernel(self) -> Result<Self> {
        match self {
            Self::Dense(weight) => {
                if !Context::detected().fused {
                    return Err(invalid("float matrix CPU features are unavailable"));
                }
                Ok(Self::Fused(fused::FusedMatrix::new(weight)?))
            }
            matrix => Ok(matrix),
        }
    }
    pub(super) fn estimated_bytes(&self) -> usize {
        match self {
            Self::Dense(m) => m.estimated_bytes(),
            Self::Fused(m) => m.estimated_bytes(),
            Self::Quantized(m) => m.estimated_bytes(),
            #[cfg(target_arch = "x86_64")]
            Self::Quantized8(m) => m.estimated_bytes(),
        }
    }
    pub(super) fn workspace_elements(&self) -> usize {
        if self.is_quantized() {
            self.rows().max(4).saturating_add(self.cols().max(4))
        } else {
            0
        }
    }
    pub(super) fn add_mode() -> Accumulate {
        Accumulate::Add {
            fused: Context::detected().fused,
        }
    }
    pub(super) fn quantized_product_into(
        &self,
        input: &[f32],
        output: &mut [f32],
        mode: Accumulate,
        workspace: &mut Workspace,
    ) -> Result<()> {
        match self {
            Self::Quantized(m) => m.product_into(input, output, mode, workspace),
            #[cfg(target_arch = "x86_64")]
            Self::Quantized8(m) => m.product_into(input, output, mode, workspace),
            Self::Dense(_) | Self::Fused(_) => Err(invalid(
                "integer product requires retained quantized coefficients",
            )),
        }
    }
    pub(super) fn product_into(
        &self,
        input: &[f32],
        output: &mut [f32],
        mode: Accumulate,
        workspace: &mut Workspace,
    ) -> Result<()> {
        match self {
            Self::Fused(matrix) => matrix.product_into(input, output, mode),
            _ => self.quantized_product_into(input, output, mode, workspace),
        }
    }
}
