use std::{collections::HashMap, sync::OnceLock};

const BASES: [u64; 12] = [
    0x0dcd59189d5a0f24,
    0x59346d79970ca21e,
    0xcf3519b773b767bf,
    0x562c8e41fb7fbee2,
    0xbd6d25457e1ed24e,
    0x0123456789abcdef,
    0x76543210fedcba98,
    0x02468aceeca86420,
    0xeca864202468acee,
    0x70556f5965766947,
    0x5a4d5a4d5a4d5a4d,
    0x000000626d6f6379,
];
const TAGS: &[&str] = &[
    "prim0", "prim1", "prim2", "prim3", "prim4", "prim5", "cmpg0", "cmpg1", "cmpu0", "cmpu1",
    "cmpno", "modl0", "modl1", "modl2", "modl3", "modl4", "modl5", "modl6", "modl7", "modl8",
    "modl9", "modl10", "moda0", "moda1", "moda2", "moda3", "moda4", "moda5", "moda6", "moda7",
    "moda8", "moda9", "modm0", "modm1", "modm2", "modm3", "modm4", "modm5", "modm6", "modm7",
    "modm8", "modm9", "_lmbrt1", "_lmce0", "_ttscs1", "_teefv0", "_taffv0", "_taltv0", "_dctov0",
    "_phenc1", "_phenc0", "_mlfmt0", "_psv1", "_psv2", "_vocfv1", "_vocfv2", "_stbkv0", "_stbkv1",
    "_wrqv0", "_wgv0", "_ppusv0", "_ppdsv0", "_ncwnv0", "_nbfmv0", "_nbcgv0", "_mcolv0", "_gnc1v0",
    "_gaolv0", "_cosev0", "_lmbrt0", "_lmhw0", "_ac2in1", "_ac2in0", "_mlien0", "_ttsdf0",
    "_ttsdf1", "_ttsdt0", "_ttsdt1", "_ttsfe0", "_ttste0", "_didsv0", "_imprd1", "_imprd0",
    "_imkrn0", "_ltvfl0", "_ltvfl1", "_nhvpl2", "_nhvpl3", "_ttsna0", "_ardev0", "_gstkv0",
    "_mspfv0",
];

pub(super) fn resolve(identity: u64) -> String {
    static TABLE: OnceLock<HashMap<u64, &'static str>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = HashMap::new();
        for tag in TAGS {
            for base in BASES {
                let hash = tag
                    .as_bytes()
                    .iter()
                    .take(7)
                    .fold(base, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3));
                if let Some(old) = table.insert(hash, *tag) {
                    assert_eq!(old, *tag, "DNNI alias collision");
                }
            }
        }
        table
    });
    table
        .get(&identity)
        .map_or_else(|| format!("#{identity:016x}"), |tag| (*tag).to_owned())
}
