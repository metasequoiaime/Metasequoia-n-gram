use super::*;

pub(super) fn reference_attention(
    model: &SentenceModel,
    q: &[f32],
    k: &[f32],
    v: &[f32],
    rows: usize,
    offset: usize,
) -> Vec<f32> {
    let n_embd = model.config.n_embd;
    let heads = model.config.n_head;
    let width = n_embd / heads;
    let scale = 1.0 / (width as f32).sqrt();
    let total = k.len() / n_embd;
    let mut out = vec![0.0; rows * n_embd];
    let mut scores = vec![0.0; total];
    for row in 0..rows {
        // 查询只能看到绝对位置 offset + row 及之前的位置。
        let visible = offset + row + 1;
        for head in 0..heads {
            let base = head * width;
            let query = &q[row * n_embd + base..row * n_embd + base + width];
            let mut highest = f32::NEG_INFINITY;
            for (step, score) in scores[..visible].iter_mut().enumerate() {
                let key = &k[step * n_embd + base..step * n_embd + base + width];
                *score = dot(query, key) * scale;
                highest = highest.max(*score);
            }
            let mut sum = 0.0;
            for score in &mut scores[..visible] {
                *score = (*score - highest).exp();
                sum += *score;
            }
            let target = &mut out[row * n_embd + base..row * n_embd + base + width];
            for (step, &score) in scores[..visible].iter().enumerate() {
                let weight = score / sum;
                let value = &v[step * n_embd + base..step * n_embd + base + width];
                for (slot, &item) in target.iter_mut().zip(value) {
                    *slot += weight * item;
                }
            }
        }
    }
    out
}

fn values(width: usize, rows: usize) -> Vec<f32> {
    (0..width * rows)
        .map(|i| {
            if i % 11 == 0 {
                -0.0
            } else {
                ((i * 7 % 31) as f32 - 15.0) / 16.0
            }
        })
        .collect()
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn warmed_attention_keeps_output_and_scores() {
    let model = super::feed_forward_scratch_tests::model(8, 1, true);
    let mut scratch = AttentionScratch::default();
    let q = values(8, 4);
    let kv = values(8, 7);
    model.attention_into(&q, &kv, &kv, 4, 3, &mut scratch);
    let pointers = (scratch.output.as_ptr(), scratch.scores.as_ptr());
    let capacities = (scratch.output.capacity(), scratch.scores.capacity());
    model.attention_into(&q, &kv, &kv, 4, 3, &mut scratch);
    assert_eq!(
        pointers,
        (scratch.output.as_ptr(), scratch.scores.as_ptr()),
        "同形状 attention 仍替换已有缓冲"
    );
    assert_eq!(
        capacities,
        (scratch.output.capacity(), scratch.scores.capacity())
    );
}

#[test]
fn attention_reuse_matches_legacy_bits_and_clears_stale_output() {
    for (width, heads) in [(1, 1), (7, 1), (8, 2), (9, 3), (192, 3), (768, 12)] {
        let mut model = super::feed_forward_scratch_tests::model(width, 1, true);
        model.config.n_head = heads;
        let mut scratch = AttentionScratch::default();
        for rows in [0, 1, 4, 12, 4, 1, 0] {
            for offset in [0, 3, 31] {
                let q = values(width, rows);
                let k = values(width, rows + offset);
                let mut v = k.clone();
                v.reverse();
                let expected = reference_attention(&model, &q, &k, &v, rows, offset);
                scratch.output.fill(f32::NAN);
                scratch.scores.fill(f32::NAN);
                model.attention_into(&q, &k, &v, rows, offset, &mut scratch);
                assert_eq!(
                    bits(&expected),
                    bits(&scratch.output),
                    "width={width} heads={heads} rows={rows} offset={offset}"
                );
                assert_eq!(scratch.output.len(), rows * width);
                assert_eq!(scratch.scores.len(), rows + offset);
            }
        }
    }
}

#[inline(never)]
fn old_chain(model: &SentenceModel, q: &[f32], kv: &[f32], rows: usize, offset: usize) -> Vec<f32> {
    let mut x = q.to_vec();
    for _ in 0..6 {
        let attended = reference_attention(model, &x, kv, kv, rows, offset);
        for (slot, value) in x.iter_mut().zip(attended) {
            *slot += value;
        }
    }
    x
}

#[inline(never)]
fn new_chain(model: &SentenceModel, q: &[f32], kv: &[f32], rows: usize, offset: usize) -> Vec<f32> {
    let mut x = q.to_vec();
    let mut scratch = AttentionScratch::default();
    for _ in 0..6 {
        model.attention_into(&x, kv, kv, rows, offset, &mut scratch);
        for (slot, &value) in x.iter_mut().zip(&scratch.output) {
            *slot += value;
        }
    }
    x
}

type Chain = fn(&SentenceModel, &[f32], &[f32], usize, usize) -> Vec<f32>;
fn elapsed(
    chain: Chain,
    model: &SentenceModel,
    q: &[f32],
    kv: &[f32],
    rows: usize,
    offset: usize,
    repeats: usize,
) -> f64 {
    use std::hint::black_box;
    let start = std::time::Instant::now();
    for _ in 0..repeats {
        drop(black_box(chain(
            black_box(model),
            black_box(q),
            black_box(kv),
            rows,
            offset,
        )));
    }
    start.elapsed().as_secs_f64()
}

#[test]
#[ignore = "独占运行完整六层 attention 配对预算，包含工作区构造析构"]
fn six_layer_attention_paired_budget() {
    let mut failures = Vec::new();
    for width in [192, 768] {
        let mut model = super::feed_forward_scratch_tests::model(width, 1, true);
        model.config.n_head = if width == 192 { 3 } else { 12 };
        for rows in [1, 4, 12] {
            for offset in [0, 19] {
                let q = values(width, rows);
                let kv = values(width, rows + offset);
                assert_eq!(
                    bits(&old_chain(&model, &q, &kv, rows, offset)),
                    bits(&new_chain(&model, &q, &kv, rows, offset))
                );
                let mut repeats = 1;
                while elapsed(old_chain, &model, &q, &kv, rows, offset, repeats) < 0.010 {
                    repeats *= 2;
                }
                let mut ratios = Vec::new();
                for pair in 0..40 {
                    let (old, new) = if pair % 2 == 0 {
                        let old = elapsed(old_chain, &model, &q, &kv, rows, offset, repeats);
                        (
                            old,
                            elapsed(new_chain, &model, &q, &kv, rows, offset, repeats),
                        )
                    } else {
                        let new = elapsed(new_chain, &model, &q, &kv, rows, offset, repeats);
                        (
                            elapsed(old_chain, &model, &q, &kv, rows, offset, repeats),
                            new,
                        )
                    };
                    ratios.push(new / old);
                }
                ratios.sort_by(f64::total_cmp);
                let ratio = ratios[19];
                let budget = if width == 192 && rows == 1 {
                    0.99
                } else {
                    1.03
                };
                println!("width={width} rows={rows} offset={offset} repeats={repeats} paired_ratio_p50={ratio:.4} budget={budget:.2}");
                if ratio >= budget {
                    failures.push((width, rows, offset, ratio, budget));
                }
            }
        }
    }
    assert!(failures.is_empty(), "完整 attention 预算失败：{failures:?}");
}
