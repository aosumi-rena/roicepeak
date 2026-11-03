//! Explicit local model paths; no installed-product or application-data discovery.
mod resources;
use crate::{
    Result, dnni,
    graph::Budget,
    invalid,
    language::CharacterCache,
    phonemes::Token,
    stylebook::Offsets,
    synthesis::{Edits, Models, Parameters, Render, Synthesizer},
};
pub use resources::{CharacterCacheResources, ModelResources};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
pub struct Manifest {
    pub synth: Synthesizer,
    pub weights: Vec<f32>,
    pub parameters: Parameters,
    pub edits: Edits,
}
fn bytes(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > limit as u64 {
        return Err(invalid("model input must be a bounded regular file"));
    }
    let mut out = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut out)?;
    if out.len() > limit {
        return Err(invalid("input grew beyond byte limit"));
    }
    Ok(out)
}
impl Manifest {
    /// Retained heap charge, excluding the inline Manifest itself.
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.synth.estimated_bytes(),
            crate::memory::values(&self.weights),
            self.edits.estimated_bytes(),
        ])
    }

    pub fn render(&self, tokens: &[Token], budget: &mut Budget) -> Result<Render> {
        self.synth
            .run_edited(tokens, &self.weights, &self.parameters, &self.edits, budget)
    }
    pub fn render_with_prediction(
        &self,
        tokens: &[Token],
        predict: &[Vec<bool>],
        budget: &mut Budget,
    ) -> Result<Render> {
        let mut edits = self.edits.clone();
        edits.predict = Some(predict.to_vec());
        self.synth
            .run_edited(tokens, &self.weights, &self.parameters, &edits, budget)
    }
    pub fn read(path: &Path) -> Result<Self> {
        let text = String::from_utf8(bytes(path, 65536)?)
            .map_err(|_| invalid("model manifest must be UTF-8"))?;
        let mut fields = BTreeMap::new();
        for line in text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        {
            let (key, value) = line
                .split_once('\t')
                .ok_or_else(|| invalid("manifest entries require key TAB value"))?;
            if ![
                "pe",
                "lm",
                "lm_cache_desc",
                "lm_cache_keys",
                "lm_cache_data",
                "im",
                "acc2int",
                "am",
                "vocoder",
                "stylebook",
                "spkvec",
                "speaker",
                "weights",
                "offsets",
                "phonemes",
                "speed",
                "pitch",
                "pause",
                "volume",
                "fixed_seeding",
                "invert_intonation",
                "accent_codes",
                "pitch_edits",
                "duration_edits",
                "seed_base",
                "seed_variations",
                "posteffect_eq_p",
                "posteffect_eq_ap",
            ]
            .contains(&key)
                || fields.insert(key, value).is_some()
            {
                return Err(invalid("unknown or duplicate model manifest key"));
            }
        }
        let required = |key: &str| {
            fields
                .get(key)
                .copied()
                .ok_or_else(|| invalid(format!("missing manifest key {key}")))
        };
        let base = path.parent().unwrap_or(Path::new("."));
        let resolve = |s: &str| {
            let p = PathBuf::from(s);
            if p.is_absolute() { p } else { base.join(p) }
        };
        let mut data = Vec::new();
        for key in ["pe", "lm", "im", "acc2int", "am", "vocoder", "stylebook"] {
            data.push(bytes(&resolve(required(key)?), dnni::MAX_BYTES)?);
        }
        let roots = data
            .iter()
            .map(|b| dnni::parse(b))
            .collect::<Result<Vec<_>>>()?;
        let speaker = fields
            .get("speaker")
            .unwrap_or(&"0")
            .parse::<usize>()
            .map_err(|_| invalid("invalid manifest speaker"))?;
        let offsets = fields
            .get("offsets")
            .map(|value| {
                let nums = value
                    .split(',')
                    .map(|v| {
                        v.parse::<usize>()
                            .map_err(|_| invalid("invalid manifest style offset"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                if nums.len() != 5 {
                    return Err(invalid("manifest offsets need five entries"));
                }
                let o = |i: usize| (nums[i] != 0).then_some(nums[i]);
                Ok(Offsets {
                    aperiodicity: o(0),
                    volume: o(1),
                    intonation: o(2),
                    speed: o(3),
                    tension: o(4),
                })
            })
            .transpose()?;
        let phones = fields
            .get("phonemes")
            .map(|p| {
                let value = String::from_utf8(bytes(&resolve(p), 1024 * 1024)?)
                    .map_err(|_| invalid("phoneme list must be UTF-8"))?;
                Ok::<Vec<String>, crate::Error>(
                    value.lines().map(str::to_owned).collect::<Vec<_>>(),
                )
            })
            .transpose()?
            .unwrap_or_default();
        let custom = fields
            .get("spkvec")
            .map(|p| {
                let data = bytes(&resolve(p), 4 * 1024 * 1024)?;
                if data.len() % 4 != 0 {
                    return Err(invalid("custom speaker file must contain float32 words"));
                }
                Ok::<Vec<f32>, crate::Error>(
                    data.chunks_exact(4)
                        .map(|w| f32::from_le_bytes(w.try_into().unwrap()))
                        .collect(),
                )
            })
            .transpose()?;
        let mut synth = Synthesizer::compile(
            Models {
                phonemes: &roots[0],
                language: &roots[1],
                intonation: &roots[2],
                accent: &roots[3],
                acoustic: &roots[4],
                vocoder: &roots[5],
                stylebook: &roots[6],
            },
            speaker,
            offsets,
            phones,
            custom.as_deref(),
        )?;
        let effect_bytes = |key: &str| {
            fields
                .get(key)
                .map(|path| bytes(&resolve(path), 4096 * 8))
                .transpose()
        };
        let periodic = effect_bytes("posteffect_eq_p")?.unwrap_or_default();
        let aperiodic = effect_bytes("posteffect_eq_ap")?.unwrap_or_default();
        synth.set_vocoder_effects(crate::vocoder::Effects::from_le_bytes(
            &periodic, &aperiodic,
        )?);
        let cache_keys = ["lm_cache_desc", "lm_cache_keys", "lm_cache_data"];
        let cache_count = cache_keys
            .iter()
            .filter(|key| fields.contains_key(**key))
            .count();
        if cache_count != 0 {
            if cache_count != cache_keys.len() {
                return Err(invalid(
                    "language cache requires descriptor, keys, and data paths",
                ));
            }
            let descriptor = bytes(&resolve(required(cache_keys[0])?), 65536)?;
            let keys = bytes(&resolve(required(cache_keys[1])?), 16 * 1024 * 1024)?;
            let data = bytes(
                &resolve(required(cache_keys[2])?),
                crate::tensor::MAX_ELEMENTS * 4,
            )?;
            synth.set_character_cache(CharacterCache::parse(&descriptor, &keys, &data)?)?;
        }
        let weights = if let Some(value) = fields.get("weights") {
            value
                .split(',')
                .map(|v| {
                    v.parse::<f32>()
                        .map_err(|_| invalid("invalid manifest style weight"))
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            let mut v = vec![0.0; synth.emotions().len()];
            v[0] = 1.0;
            v
        };
        let scalar = |key: &str, default: f32| {
            fields
                .get(key)
                .map(|v| {
                    v.parse::<f32>()
                        .map_err(|_| invalid("invalid manifest scalar"))
                })
                .unwrap_or(Ok(default))
        };
        let flag = |key: &str| match fields.get(key).copied().unwrap_or("0") {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(invalid("manifest flag requires 0 or 1")),
        };
        let parameters = Parameters {
            speed: scalar("speed", 100.0)?,
            pitch: scalar("pitch", 0.0)?,
            pause: scalar("pause", 100.0)?,
            volume: scalar("volume", 100.0)?,
            fixed_seeding: flag("fixed_seeding")?,
            invert_intonation: flag("invert_intonation")?,
        };
        let accent_codes = fields
            .get("accent_codes")
            .map(|text| {
                text.split(',')
                    .map(|value| {
                        let result = if let Some(hex) = value.strip_prefix("0x") {
                            u32::from_str_radix(hex, 16)
                        } else {
                            value.parse::<u32>()
                        };
                        let code = result.map_err(|_| invalid("invalid manifest accent code"))?;
                        if !crate::prosody::CODES.contains(&code) {
                            return Err(invalid("unsupported manifest accent code"));
                        }
                        Ok(code)
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let pitch = indexed_edits(fields.get("pitch_edits").copied())?
            .into_iter()
            .map(|(index, value)| {
                let value = value as f32;
                if !value.is_finite() {
                    return Err(invalid("manifest pitch edit exceeds float32"));
                }
                Ok((index, value))
            })
            .collect::<Result<_>>()?;
        let edits = Edits {
            accent_codes,
            predict: None,
            seed_base: fields
                .get("seed_base")
                .map(|value| {
                    value
                        .parse::<i32>()
                        .map_err(|_| invalid("manifest seed base requires int32"))
                })
                .transpose()?
                .unwrap_or(0),
            seed_variations: fields
                .get("seed_variations")
                .map(|value| {
                    value
                        .split(',')
                        .map(|n| {
                            n.parse::<i32>()
                                .map_err(|_| invalid("manifest seed variation requires int32"))
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?,
            pitch,
            duration: indexed_edits(fields.get("duration_edits").copied())?,
            authored_duration: Default::default(),
            intonation_delta: Default::default(),
            boundary_silence: crate::synthesis::BoundarySilence::Predicted,
            scalar_layers: None,
            value_policy: Default::default(),
        };
        Ok(Self {
            synth,
            weights,
            parameters,
            edits,
        })
    }
}

fn indexed_edits(text: Option<&str>) -> Result<BTreeMap<usize, f64>> {
    let mut out = BTreeMap::new();
    let Some(text) = text else {
        return Ok(out);
    };
    for pair in text.split(',') {
        let (index, value) = pair
            .split_once(':')
            .ok_or_else(|| invalid("manifest edit requires index:value"))?;
        let index = index
            .parse::<usize>()
            .map_err(|_| invalid("invalid manifest edit index"))?;
        let value = value
            .parse::<f64>()
            .map_err(|_| invalid("invalid manifest edit value"))?;
        if !value.is_finite() || out.insert(index, value).is_some() {
            return Err(invalid("nonfinite or duplicate manifest edit"));
        }
    }
    Ok(out)
}
