//! Accent analysis and duration/intonation prediction on explicit linguistic rows.
use crate::{
    Result,
    dnni::Node,
    graph::{Budget, Graph},
    invalid,
    lingual::Lingual,
    normalization::Normalizer,
    sequence::{broadcast, concatenate},
    tensor::{Tensor, u32_at},
};

pub const CODES: [u32; 11] = [
    0x1000, 0x2000, 0x2001, 0x3001, 0x3002, 0x3003, 0x3004, 0x3000, 0x4000, 0x4001, 0x4002,
];
#[derive(Clone, Debug)]
pub struct FeatureFormat {
    pub japanese: u32,
    pub slots: [usize; 11],
}
impl FeatureFormat {
    pub fn slot(&self, code: u32) -> Result<usize> {
        CODES
            .iter()
            .position(|c| *c == code)
            .map(|i| self.slots[i])
            .ok_or_else(|| invalid("unknown prosody code"))
    }
    fn validate(&self, width: usize) -> Result<()> {
        if self.slots.iter().any(|i| *i >= width) {
            return Err(invalid("prosody flag outside feature row"));
        }
        Ok(())
    }
    pub fn decode(&self, features: &[f32]) -> Result<u32> {
        self.validate(features.len())?;
        CODES
            .iter()
            .zip(self.slots)
            .find(|(_, i)| features[*i] >= 0.5)
            .map(|(code, _)| *code)
            .ok_or_else(|| invalid("missing prosody flag"))
    }
    /// The stored format clears the half-open interval, including any interior slots.
    /// Its highest flag can consequently survive recoding; this is deliberate.
    pub fn recode(&self, input: &Tensor, codes: &[u32]) -> Result<Tensor> {
        if input.cols() < 3 || input.rows() != codes.len() {
            return Err(invalid("prosody recode dimensions"));
        }
        self.validate(input.cols() - 2)?;
        let mut out = input.clone();
        let low = *self.slots.iter().min().unwrap() + 2;
        let high = *self.slots.iter().max().unwrap() + 2;
        for (t, code) in codes.iter().enumerate() {
            out.row_mut(t)[low..high].fill(0.0);
            out.row_mut(t)[2 + self.slot(*code)?] = 1.0;
        }
        Ok(out)
    }
}
enum Encoder {
    Raw,
    Learned(Lingual),
}
impl Encoder {
    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Raw => 0,
            Self::Learned(value) => value.estimated_bytes(),
        }
    }
}
impl Encoder {
    fn run(&self, input: &Tensor, zero: &[usize], budget: &mut Budget) -> Result<Tensor> {
        if input.cols() < 3 {
            return Err(invalid("linguistic row needs IDs and features"));
        }
        let mut input = input.clone();
        for t in 0..input.rows() {
            if input.row(t)[..2]
                .iter()
                .any(|v| *v < 0.0 || v.fract() != 0.0)
            {
                return Err(invalid("linguistic IDs must be nonnegative integers"));
            }
            for col in zero {
                *input
                    .row_mut(t)
                    .get_mut(2 + col)
                    .ok_or_else(|| invalid("zero flag outside feature row"))? = 0.0;
            }
        }
        match self {
            Self::Raw => Tensor::new(
                input.rows(),
                input.cols() - 2,
                (0..input.rows())
                    .flat_map(|t| input.row(t)[2..].iter().copied())
                    .collect(),
            ),
            Self::Learned(e) => e.run(&input, budget),
        }
    }
}
struct Kernel {
    conv: Graph,
    recurrent: Graph,
    classifier: Graph,
    pitch: Graph,
    duration: Graph,
}
impl Kernel {
    fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.conv.estimated_bytes(),
            self.recurrent.estimated_bytes(),
            self.classifier.estimated_bytes(),
            self.pitch.estimated_bytes(),
            self.duration.estimated_bytes(),
        ])
    }
}
pub struct IntonationModel {
    encoder: Encoder,
    normalizer: Normalizer,
    kernels: Vec<Kernel>,
    format: FeatureFormat,
}
pub struct Analysis {
    pub codes: Vec<u32>,
    pub hidden: Vec<Tensor>,
}
fn version(root: &Node<'_>, prefix: &str, legacy: usize, modern: usize) -> Result<bool> {
    let is_modern = root.tag == format!("{prefix}1");
    if (!is_modern && root.tag != format!("{prefix}0"))
        || u32_at(root.payload, 0)? != u32::from(is_modern)
        || root.children.len() != if is_modern { modern } else { legacy }
    {
        return Err(invalid("unsupported prosody root structure"));
    }
    Ok(is_modern)
}
impl IntonationModel {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.encoder.estimated_bytes(),
            self.normalizer.estimated_bytes(),
            crate::memory::nested(&self.kernels, Kernel::estimated_bytes),
        ])
    }

    pub fn compile(root: &Node<'_>, format: FeatureFormat) -> Result<Self> {
        let modern = version(root, "_imprd", 3, 4)?;
        if root.payload.len() != 5 || root.payload[4] != 0 {
            return Err(invalid("unsupported intonation flags"));
        }
        let offset = usize::from(modern);
        let mut kernels = Vec::new();
        for k in &root.children[offset + 2].children {
            if k.tag != "_imkrn0" || k.payload != [0] || k.children.len() != 5 {
                return Err(invalid("intonation kernel structure"));
            }
            let c = &k.children;
            kernels.push(Kernel {
                conv: Graph::compile(&c[0])?,
                recurrent: Graph::compile(&c[1])?,
                classifier: Graph::compile(&c[2])?,
                pitch: Graph::compile(&c[3])?,
                duration: Graph::compile(&c[4])?,
            });
        }
        if kernels.is_empty() {
            return Err(invalid("no intonation kernels"));
        }
        Ok(Self {
            encoder: if modern {
                Encoder::Learned(Lingual::compile(&root.children[0])?)
            } else {
                Encoder::Raw
            },
            normalizer: Normalizer::from_node(&root.children[offset + 1])?,
            kernels,
            format,
        })
    }
    pub fn analyze(
        &self,
        input: &Tensor,
        lm: &Tensor,
        word_index: &[usize],
        budget: &mut Budget,
    ) -> Result<Analysis> {
        if input.cols() < 3
            || word_index.len() != input.rows()
            || word_index.iter().any(|i| *i >= lm.rows())
        {
            return Err(invalid("intonation input/word mapping"));
        }
        self.format.validate(input.cols() - 2)?;
        let selected = Tensor::new(
            input.rows(),
            lm.cols(),
            word_index
                .iter()
                .flat_map(|i| lm.row(*i).iter().copied())
                .collect(),
        )?;
        let encoded = self.encoder.run(input, &[], budget)?;
        let joined = concatenate(&[&encoded, &selected], budget)?;
        let mut hidden = Vec::new();
        let mut scores = Tensor::zeros(input.rows(), 3)?;
        for k in &self.kernels {
            let h = k
                .recurrent
                .run_with(&k.conv.run_with(&joined, budget)?, budget)?;
            let s = k.classifier.run_with(&h, budget)?;
            if s.rows() != input.rows() || s.cols() != 3 {
                return Err(invalid("accent classifier shape"));
            }
            for t in 0..input.rows() {
                for (sum, v) in scores.row_mut(t).iter_mut().zip(s.row(t)) {
                    *sum += v;
                }
            }
            hidden.push(h);
        }
        let mut codes = Vec::with_capacity(input.rows());
        for t in 0..input.rows() {
            let f = &input.row(t)[2..];
            codes.push(if f[self.format.slots[0]] >= 0.5 {
                CODES[0]
            } else if input.row(t)[1] == self.format.japanese as f32 {
                // Strict comparison preserves the first class for equal scores.
                let mut class = 0;
                for i in 1..3 {
                    if scores.row(t)[i] > scores.row(t)[class] {
                        class = i;
                    }
                }
                CODES[class]
            } else {
                self.format.decode(f)?
            });
        }
        Ok(Analysis { codes, hidden })
    }
    pub fn predict(
        &self,
        analysis: &Analysis,
        speaker: &[f32],
        style: &[f32],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let rows = analysis.codes.len();
        if analysis.hidden.len() != self.kernels.len() {
            return Err(invalid("intonation hidden count"));
        }
        let cond: Vec<_> = speaker.iter().chain(style).copied().collect();
        let cond = broadcast(&cond, rows, budget)?;
        let mut out = Tensor::zeros(rows, 2)?;
        for (k, h) in self.kernels.iter().zip(&analysis.hidden) {
            let z = concatenate(&[h, &cond], budget)?;
            let pitch = k.pitch.run_with(&z, budget)?;
            let duration = k.duration.run_with(&z, budget)?;
            if pitch.rows() != rows
                || duration.rows() != rows
                || pitch.cols() != 1
                || duration.cols() != 1
            {
                return Err(invalid("intonation head shape"));
            }
            for t in 0..rows {
                out.row_mut(t)[0] += pitch.row(t)[0];
                out.row_mut(t)[1] += duration.row(t)[0];
            }
        }
        let count = self.kernels.len() as f32;
        let logs = Tensor::new(rows, 1, (0..rows).map(|t| out.row(t)[1] / count).collect())?;
        let logs = self.normalizer.denormalize(&logs)?;
        for t in 0..rows {
            out.row_mut(t)[0] /= count;
            let frames = (f64::from(logs.row(t)[0].exp() - 0.1) + 0.5)
                .floor()
                .max(1.0);
            if !frames.is_finite() || frames > 1_000_000.0 {
                return Err(invalid("predicted duration out of bounds"));
            }
            out.row_mut(t)[1] = frames as f32;
        }
        out.ensure_finite()?;
        Ok(out)
    }
}
pub struct AccentToIntonation {
    encoder: Encoder,
    accents: Graph,
    conv: Graph,
    recurrent: Graph,
    head: Graph,
    format: FeatureFormat,
}
impl AccentToIntonation {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.encoder.estimated_bytes(),
            self.accents.estimated_bytes(),
            self.conv.estimated_bytes(),
            self.recurrent.estimated_bytes(),
            self.head.estimated_bytes(),
        ])
    }

    pub fn compile(root: &Node<'_>, format: FeatureFormat) -> Result<Self> {
        let modern = version(root, "_ac2in", 5, 6)?;
        if root.payload.len() != 4 {
            return Err(invalid("accent-to-intonation payload"));
        }
        let c = &root.children;
        let o = usize::from(modern);
        Ok(Self {
            encoder: if modern {
                Encoder::Learned(Lingual::compile(&c[0])?)
            } else {
                Encoder::Raw
            },
            accents: Graph::compile(&c[o + 1])?,
            conv: Graph::compile(&c[o + 2])?,
            recurrent: Graph::compile(&c[o + 3])?,
            head: Graph::compile(&c[o + 4])?,
            format,
        })
    }
    pub fn run(
        &self,
        input: &Tensor,
        codes: &[u32],
        speaker: &[f32],
        style: &[f32],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if input.cols() < 3 || input.rows() != codes.len() {
            return Err(invalid("accent-to-intonation inputs"));
        }
        self.format.validate(input.cols() - 2)?;
        let x = self.encoder.run(input, &self.format.slots[1..3], budget)?;
        let x = self.conv.run_with(&x, budget)?;
        let mut ids = Tensor::zeros(input.rows(), 1)?;
        let mut active = vec![false; input.rows()];
        for (t, code) in codes.iter().enumerate() {
            if input.row(t)[1] == self.format.japanese as f32 && *code != CODES[0] {
                ids.row_mut(t)[0] = match *code {
                    0x2000 => 1.0,
                    0x2001 => 2.0,
                    _ => return Err(invalid("Japanese accent must be L or H")),
                };
                active[t] = true;
            }
        }
        let mut accent = self.accents.run_with(&ids, budget)?;
        for (t, active) in active.iter().enumerate() {
            if !active {
                accent.row_mut(t).fill(0.0);
            }
        }
        let h = self
            .recurrent
            .run_with(&concatenate(&[&x, &accent], budget)?, budget)?;
        let cond: Vec<_> = speaker.iter().chain(style).copied().collect();
        let cond = broadcast(&cond, input.rows(), budget)?;
        let out = self
            .head
            .run_with(&concatenate(&[&h, &cond], budget)?, budget)?;
        if out.rows() != input.rows() || out.cols() != 1 {
            return Err(invalid("accent-to-intonation output"));
        }
        Ok(out)
    }
}
pub struct Prosody {
    im: IntonationModel,
    accent: AccentToIntonation,
    format: FeatureFormat,
}
impl Prosody {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([self.im.estimated_bytes(), self.accent.estimated_bytes()])
    }

    pub fn compile(im: &Node<'_>, accent: &Node<'_>, format: FeatureFormat) -> Result<Self> {
        Ok(Self {
            im: IntonationModel::compile(im, format.clone())?,
            accent: AccentToIntonation::compile(accent, format.clone())?,
            format,
        })
    }
    /// Output columns are prosody code, intonation, and whole duration frames.
    pub fn run(
        &self,
        input: &Tensor,
        lm: &Tensor,
        words: &[usize],
        edited: Option<&[u32]>,
        speaker_im: &[f32],
        speaker_accent: &[f32],
        style: &[f32],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        self.run_with_prediction(
            input,
            lm,
            words,
            None,
            edited,
            speaker_im,
            speaker_accent,
            style,
            budget,
        )
    }
    /// False mask entries retain the supplied linguistic accent code.
    /// The standalone reference omits application-level mask selection.
    pub fn run_with_prediction(
        &self,
        input: &Tensor,
        lm: &Tensor,
        words: &[usize],
        predict: Option<&[bool]>,
        edited: Option<&[u32]>,
        speaker_im: &[f32],
        speaker_accent: &[f32],
        style: &[f32],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if predict.is_some_and(|mask| mask.len() != input.rows()) {
            return Err(invalid("prosody prediction mask dimensions"));
        }
        let analysis = self.im.analyze(input, lm, words, budget)?;
        let policy = predict
            .map(|mask| {
                mask.iter()
                    .enumerate()
                    .map(|(t, predict)| {
                        if *predict {
                            Ok(analysis.codes[t])
                        } else {
                            self.format.decode(&input.row(t)[2..])
                        }
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let codes = edited.unwrap_or_else(|| policy.as_deref().unwrap_or(&analysis.codes));
        let changed = codes != analysis.codes;
        let features = if changed {
            self.format.recode(input, codes)?
        } else {
            input.clone()
        };
        let refreshed = if changed {
            Some(self.im.analyze(&features, lm, words, budget)?)
        } else {
            None
        };
        let predicted = self.im.predict(
            refreshed.as_ref().unwrap_or(&analysis),
            speaker_im,
            style,
            budget,
        )?;
        let base = self
            .accent
            .run(&features, &analysis.codes, speaker_accent, style, budget)?;
        let edit = self
            .accent
            .run(&features, codes, speaker_accent, style, budget)?;
        Tensor::new(
            input.rows(),
            3,
            (0..input.rows())
                .flat_map(|t| {
                    let shifted =
                        logit(predicted.row(t)[0]) - logit(base.row(t)[0]) + logit(edit.row(t)[0]);
                    [
                        codes[t] as f32,
                        1.0 / (1.0 + (-shifted).exp()),
                        predicted.row(t)[1],
                    ]
                })
                .collect(),
        )
    }
}
pub fn logit(value: f32) -> f32 {
    let v = f64::from(value);
    (v / (1.0 - v).max(1e-100)).max(1e-100).ln() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recoding_preserves_highest_slot_and_first_set_flag() {
        let fmt = FeatureFormat {
            japanese: 0,
            slots: [0, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        let input = Tensor::new(1, 5, vec![0.0, 0.0, 0.0, 0.0, 1.0]).unwrap();
        let out = fmt.recode(&input, &[0x2000]).unwrap();
        assert_eq!(&out.row(0)[2..], &[0.0, 1.0, 1.0]);
        assert_eq!(fmt.decode(&out.row(0)[2..]).unwrap(), 0x2000);
        assert!(fmt.recode(&input, &[0xffff]).is_err());
    }
}
