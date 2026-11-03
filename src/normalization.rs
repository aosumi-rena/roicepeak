//! Stored statistics with explicit forward/inverse direction.
use crate::{Result, dnni::Node, invalid, tensor::Tensor};

#[derive(Debug)]
pub enum Normalizer {
    Range {
        minimum: Option<Vec<f32>>,
        maximum: Option<Vec<f32>>,
        low: f32,
        high: f32,
    },
    Standard {
        mean: Vec<f32>,
        deviation: Vec<f32>,
        reciprocal: Option<Vec<f32>>,
    },
}
fn vector(n: &Node<'_>) -> Result<Vec<f32>> {
    if n.tag != "prim1" {
        return Err(invalid("normalizer expected vector"));
    }
    Ok(n.tensor()?.values().to_vec())
}
impl Normalizer {
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        match self {
            Self::Range {
                minimum, maximum, ..
            } => m::sum([
                m::optional(minimum, m::values),
                m::optional(maximum, m::values),
            ]),
            Self::Standard {
                mean,
                deviation,
                reciprocal,
            } => m::sum([
                m::values(mean),
                m::values(deviation),
                m::optional(reciprocal, m::values),
            ]),
        }
    }

    pub fn channels(&self, range: std::ops::Range<usize>) -> Result<Self> {
        let slice = |v: &[f32]| {
            v.get(range.clone())
                .map(<[f32]>::to_vec)
                .ok_or_else(|| invalid("normalizer channel range"))
        };
        match self {
            Self::Range {
                minimum,
                maximum,
                low,
                high,
            } => Ok(Self::Range {
                minimum: minimum.as_ref().map(|v| slice(v)).transpose()?,
                maximum: maximum.as_ref().map(|v| slice(v)).transpose()?,
                low: *low,
                high: *high,
            }),
            Self::Standard {
                mean,
                deviation,
                reciprocal,
            } => Ok(Self::Standard {
                mean: slice(mean)?,
                deviation: slice(deviation)?,
                reciprocal: reciprocal.as_ref().map(|v| slice(v)).transpose()?,
            }),
        }
    }
    pub fn from_node(n: &Node<'_>) -> Result<Self> {
        if !n.payload.is_empty() {
            return Err(invalid("unexpected normalizer payload"));
        }
        match n.tag.as_str() {
            "cmpu0" => {
                if n.children.len() != 3 {
                    return Err(invalid("range normalizer child count"));
                }
                let optional = |n: &Node<'_>| {
                    if n.tag == "cmpno" && n.payload.is_empty() && n.children.is_empty() {
                        Ok(None)
                    } else {
                        vector(n).map(Some)
                    }
                };
                let minimum = optional(&n.children[0])?;
                let maximum = optional(&n.children[1])?;
                if minimum.is_some() != maximum.is_some()
                    || minimum
                        .as_ref()
                        .zip(maximum.as_ref())
                        .is_some_and(|(a, b)| a.len() != b.len())
                {
                    return Err(invalid("range statistics differ"));
                }
                let bounds = vector(&n.children[2])?;
                if bounds.len() != 2 || bounds[0] == bounds[1] {
                    return Err(invalid("range bounds"));
                }
                Ok(Self::Range {
                    minimum,
                    maximum,
                    low: bounds[0],
                    high: bounds[1],
                })
            }
            "cmpu1" => {
                if !matches!(n.children.len(), 2 | 3) {
                    return Err(invalid("standard normalizer child count"));
                }
                let mean = vector(&n.children[0])?;
                let deviation = vector(&n.children[1])?;
                let reciprocal = n.children.get(2).map(vector).transpose()?;
                if mean.len() != deviation.len()
                    || reciprocal.as_ref().is_some_and(|v| v.len() != mean.len())
                {
                    return Err(invalid("standard statistics differ"));
                }
                Ok(Self::Standard {
                    mean,
                    deviation,
                    reciprocal,
                })
            }
            _ => Err(invalid("unsupported normalizer")),
        }
    }
    pub fn normalize(&self, x: &Tensor, clip: bool) -> Result<Tensor> {
        self.apply(x, false, clip)
    }
    pub fn denormalize(&self, x: &Tensor) -> Result<Tensor> {
        self.apply(x, true, false)
    }
    fn apply(&self, x: &Tensor, inverse: bool, clip: bool) -> Result<Tensor> {
        let mut values = Vec::with_capacity(x.values().len());
        match self {
            Self::Range {
                minimum,
                maximum,
                low,
                high,
            } => {
                let (Some(a), Some(b)) = (minimum, maximum) else {
                    return Err(invalid("range statistics require an explicit owner"));
                };
                if a.len() != x.cols()
                    || b.len() != x.cols()
                    || !low.is_finite()
                    || !high.is_finite()
                    || low == high
                {
                    return Err(invalid("range normalizer dimensions/bounds"));
                }
                for (i, v) in x.values().iter().enumerate() {
                    let c = i % x.cols();
                    let y = if inverse {
                        (b[c] - a[c]) * (*v - low) / (high - low) + a[c]
                    } else {
                        let q = (*v - a[c]) / ((b[c] - a[c]) + 1e-5);
                        let q = if clip { q.clamp(-1.0, 1.0) } else { q };
                        (high - low) * q + low
                    };
                    values.push(y);
                }
            }
            Self::Standard {
                mean,
                deviation,
                reciprocal,
            } => {
                if mean.len() != x.cols()
                    || deviation.len() != x.cols()
                    || reciprocal.as_ref().is_some_and(|v| v.len() != x.cols())
                {
                    return Err(invalid("standard normalizer dimensions"));
                }
                for (i, v) in x.values().iter().enumerate() {
                    let c = i % x.cols();
                    values.push(if inverse {
                        mean[c] + deviation[c] * v
                    } else {
                        let difference = *v - mean[c];
                        if let Some(reciprocal) = reciprocal {
                            difference * reciprocal[c]
                        } else {
                            difference / deviation[c]
                        }
                    });
                }
            }
        }
        Tensor::new(x.rows(), x.cols(), values)
    }
}
