//! Bounded surface-sequence alignment with deterministic operation ordering.
#[derive(Clone, Copy)]
pub struct Change {
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub kind: u8,
}
pub fn align(old: &[&str], new: &[&str]) -> Result<Vec<Change>, String> {
    let width = new.len().checked_add(1).ok_or("Alignment size overflow")?;
    let cells = old
        .len()
        .checked_add(1)
        .and_then(|n| n.checked_mul(width))
        .filter(|n| *n <= 4_194_304)
        .ok_or("Word alignment exceeds its work budget")?;
    let mut costs = vec![0u32; cells];
    let mut ops = vec![0u8; cells];
    for i in 1..=old.len() {
        costs[i * width] = i as u32;
        ops[i * width] = 3;
    }
    for j in 1..=new.len() {
        costs[j] = j as u32;
        ops[j] = 2;
    }
    for i in 1..=old.len() {
        for j in 1..=new.len() {
            let at = i * width + j;
            let equal = old[i - 1] == new[j - 1];
            let diagonal = costs[at - width - 1] + u32::from(!equal);
            let insertion = costs[at - 1] + 1;
            let deletion = costs[at - width] + 1;
            let (value, op) = if diagonal <= insertion && diagonal <= deletion {
                (diagonal, u8::from(!equal))
            } else if insertion <= diagonal && insertion <= deletion {
                (insertion, 2)
            } else {
                (deletion, 3)
            };
            costs[at] = value;
            ops[at] = op;
        }
    }
    let (mut i, mut j) = (old.len(), new.len());
    let mut changes = Vec::new();
    while i != 0 || j != 0 {
        changes.push(Change {
            old: i.checked_sub(1),
            new: j.checked_sub(1),
            kind: ops[i * width + j],
        });
        let diagonal = if i > 0 && j > 0 {
            costs[(i - 1) * width + j - 1]
        } else {
            u32::MAX
        };
        let deletion = if i > 0 {
            costs[(i - 1) * width + j]
        } else {
            u32::MAX
        };
        let insertion = if j > 0 {
            costs[i * width + j - 1]
        } else {
            u32::MAX
        };
        if diagonal <= deletion && diagonal <= insertion {
            i -= 1;
            j -= 1;
        } else if deletion < insertion {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    changes.reverse();
    let mut seen = vec![false; old.len()];
    for change in &mut changes {
        if change.kind <= 1 {
            if let Some(i) = change.old {
                if seen[i] {
                    change.kind = 2;
                } else {
                    seen[i] = true;
                }
            } else {
                change.kind = 2;
            }
        }
    }
    let mut seen = vec![false; new.len()];
    for change in &mut changes {
        if change.kind <= 1 {
            if let Some(j) = change.new {
                if seen[j] {
                    change.kind = 3;
                } else {
                    seen[j] = true;
                }
            } else {
                change.kind = 3;
            }
        }
    }
    for change in &mut changes {
        if change.kind == 2 {
            change.old = None;
        }
        if change.kind == 3 {
            change.new = None;
        }
    }
    Ok(changes)
}
