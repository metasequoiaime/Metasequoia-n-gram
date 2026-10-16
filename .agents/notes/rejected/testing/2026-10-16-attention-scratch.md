# Agent Note: 同一次前向复用 attention 输出与分数缓冲

Status: rejected — 带上文形状没有稳定收益，四个形状未通过预设配对门禁

## Problem

九键按键仍超过 16 ms；已有热路径采样有矩阵运算和申请释放。每次前缀或非空尾部前向的每层 attention 都新建输出与 softmax 分数，而同一次前向的形状不变。最新采样只覆盖词库准备，不能作为新的热路径证据。

## Proposal

在 run_prefix 和非空 run_tail_states 内各持有局部 AttentionScratch，跨层复用 output 和 scores。输出每次先清零；scores 的可见前缀每次完整重写。保留因果边界、循环、浮点分组、KV 所有权和模型缓存寿命。

## Existing decisions

已检索 attention、前向缓冲和工作区。[前馈缓冲](../../implemented/testing/2026-10-16-feed-forward-scratch.md)部分相关，继续持有前馈层决定；[最终 KV](../../implemented/testing/2026-10-16-packed-tail-kv.md)继续持有可见长度和最终 KV 构造。其拒绝跨轮工作区的理由仍有效。本片没有覆盖或翻转既有决定，没有过时提案。

## Alternatives considered

- 保留新建输出和 scores：层内状态最少、实现直接，但重复分配无缓存收益；先用完整 attention 配对验证再决定是否接入。
- 跨按键持有工作区：可以进一步复用，但扩大缓存状态和持有内存，直接评分入口也需要额外同步；本片只跨同一调用的层。
- 同时复用 QKV、query 和投影：能省更多分配，但扩大别名和覆盖边界；先独立完成 attention 的两份存储。

## Predeclared criteria

暖缓冲地址测试先红后绿；多头、边界宽度、空行、非零 offset、缩小形状与污染缓冲逐位一致；固定真实模型 KV、隐藏状态和分布一致；Rust 1.75、测试、Clippy、fmt 通过。完整六层 attention 配对测量包含工作区构造与析构，量化 192 宽单行中位比小于 0.99，其余形状小于 1.03；全部形状一次判断。附完整模型 bench 表，不把局部收益当成端到端预算达标。

## Consequences

output 和 scores 容量跨后续投影及前馈存活，持有期变长，不能宣称整次前向峰值内存下降。输出必须清零后再累加，缩小形状必须截断；scores 不可见尾部不能参与 softmax。

## Verification

实验基线为 `34cc088a7be5d426292fcd8d4d788a613966ba00`，生产代码在最终变更中全部恢复。先仅传递局部工作区并保留旧新建路径，暖缓冲地址回归真实失败（1 passed、1 failed）；写入型 helper 后两项新测试通过。Rust 1.97.1 和 1.75.0 的默认测试各为 37 passed、4 ignored，模型格式测试各 15 passed，doc tests 通过；两种编译器的定向真实模型逐位比较各 1 passed。全目标 Clippy `-D warnings`、Rustfmt、差异与笔记检查通过。独立只读审查未发现数值、因果范围或缓存寿命缺陷。

最初一次 `--include-ignored --skip six_layer` 误触发既有完整词表计时，和构建并行后主动终止；该计时不作为任何结论依据。正式正确性验证重跑默认测试和定向真实模型测试，没有把终止运行记成通过。

固定真实模型 SHA-256 为 `86ac529510cb3b4968a5e6ade83ec8080f5b34a0a681e75c362accbbd387d1c1`。Float 与 Quantized 都共用 f32 attention；局部配对覆盖 192 宽三头、768 宽十二头，各 1/4/12 行、offset 0/19。完整六层残差 attention 链计入输入复制、工作区构造和析构，旧臂至少 10 ms 校准，每形状 40 对、交替先后。输出逐位相同，全部形状采集后一次判断，不放宽门槛。

配对进程退出 101，原因仅为以下四项预算失败。每 0.5 秒监测一次 rustc、C/C++、Swift、Java 编译器及构建器，整个配对未观察到构建进程；这只说明无构建重叠，不宣称机器独占。

| 宽度 | 行数 | offset | 旧臂重复数 | 新/旧中位比 | 门槛 | 结果 |
| --- | --- | --- | --- | --- | --- | --- |
| 192 | 1 | 0 | 16384 | 0.8408 | < 0.99 | 通过 |
| 192 | 1 | 19 | 2048 | 1.0062 | < 0.99 | 失败 |
| 192 | 4 | 0 | 4096 | 0.9575 | < 1.03 | 通过 |
| 192 | 4 | 19 | 512 | 1.0311 | < 1.03 | 失败 |
| 192 | 12 | 0 | 512 | 1.0103 | < 1.03 | 通过 |
| 192 | 12 | 19 | 128 | 1.0406 | < 1.03 | 失败 |
| 768 | 1 | 0 | 8192 | 0.9398 | < 1.03 | 通过 |
| 768 | 1 | 19 | 512 | 1.0318 | < 1.03 | 失败 |
| 768 | 4 | 0 | 1024 | 0.9813 | < 1.03 | 通过 |
| 768 | 4 | 19 | 128 | 1.0296 | < 1.03 | 通过 |
| 768 | 12 | 0 | 128 | 0.9762 | < 1.03 | 通过 |
| 768 | 12 | 19 | 32 | 1.0226 | < 1.03 | 通过 |

192 宽单行无上文约快 16%，但同形状带 19 字上文为 1.0062，没有达到至少 1% 收益；另三项超过 3% 回退界限。重复申请减少不代表 attention 整体一定变快，不能拿无上文成绩代替实际有上文的评分，也不能换一个阈值把本轮变绿。本片没有证明编译器层面的回退根因；不要把推测的别名、内联或清零成本记作已确认事实。

## Full model observations

按旧、新串行运行 `reference/examples/bench.rs`，同一模型和主项目固定九键合成集 81 例，每形状 40 次。两次均退出 0，但 0.5 秒采样分别 44、27 次观察到并行构建，全部只作受干扰观察，不能归因整体差异或作为门禁通过证据。局部配对已足以否决本片，不为寻找通过结果而继续重试完整模型。

旧版 release bench SHA-256：`02f1630064e4c58e8f1f85d7143f8a56b3808e8bb8ab779f721a0054cde47980`；新版：`bf38e5d6e6542c6553818bef454d4c0b94fb0a20ab759fd13f962621aeb983da`；配对测试二进制：`6dbe362574c7d00259a401792a4d147effe02434559229e57ec2219e0582a459`。十五字列没有有效候选样本，零值不能解释成真实十五字决策。

### 旧版完整表

```text
one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.47 /  0.47   0.93 /  0.94   1.87 /  1.89   2.76 /  2.80   3.68 /  3.81   4.61 /  4.70   5.56 /  5.63   0.00 /  0.00
         3 cand   0.44 /  0.50   1.84 /  1.89   4.58 /  4.68   7.18 /  7.46  10.11 / 10.25  12.86 / 13.09  15.63 / 15.93   0.00 /  0.00
         5 cand   0.47 /  0.48   2.79 /  2.87   7.38 /  7.50  12.00 / 12.20  16.55 / 16.76  21.19 / 21.45  25.12 / 25.45   0.00 /  0.00
         9 cand   0.45 /  0.46   4.46 /  4.50  12.47 / 12.67  20.38 / 20.89  29.14 / 29.62  37.82 / 38.21  44.91 / 46.17   0.00 /  0.00

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         5 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.004  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.000 / 0.000

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.002  0.002 / 0.003  0.004 / 0.004  0.006 / 0.009  0.005 / 0.009  0.000 / 0.000
         3 cand  0.001 / 0.001  0.003 / 0.003  0.006 / 0.011  0.007 / 0.014  0.008 / 0.011  0.010 / 0.019  0.011 / 0.018  0.000 / 0.000
         5 cand  0.001 / 0.001  0.005 / 0.005  0.009 / 0.010  0.012 / 0.017  0.017 / 0.019  0.018 / 0.022  0.018 / 0.025  0.000 / 0.000
         9 cand  0.001 / 0.001  0.009 / 0.011  0.016 / 0.017  0.024 / 0.033  0.027 / 0.037  0.033 / 0.041  0.032 / 0.042  0.000 / 0.000

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.45            0.45            0.45
         2          4.92            4.47            2.46
         4         26.34           12.83            6.58
         6         64.15           21.04           10.69
         8        118.10           29.42           14.76
        10        190.16           37.55           19.02
        12        296.48           46.92           24.71
        15        282.44           47.66           18.83

slowest keystroke 47.66ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.
```

### 新版完整表

```text
one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.47 /  0.48   0.94 /  1.04   1.87 /  2.39   2.82 /  5.72   3.74 /  3.80   4.66 /  8.01   5.58 /  6.18   0.00 /  0.00
         3 cand   0.47 /  0.47   1.91 /  5.29   4.67 /  7.83   7.48 / 11.04  10.27 / 14.79  13.07 / 18.27  15.85 / 20.42   0.00 /  0.00
         5 cand   0.47 /  0.49   2.84 /  6.12   7.47 /  9.92  12.18 / 15.36  17.40 / 25.17  21.43 / 27.11  26.11 / 27.33   0.00 /  0.00
         9 cand   0.47 /  0.52   4.67 /  4.75  13.00 / 13.15  21.35 / 21.51  29.66 / 29.98  38.31 / 51.20  46.29 / 46.82   0.00 /  0.00

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         5 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.002 / 0.002  0.002 / 0.002  0.002 / 0.004  0.000 / 0.000

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.003  0.002 / 0.004  0.004 / 0.005  0.007 / 0.009  0.005 / 0.009  0.000 / 0.000
         3 cand  0.001 / 0.001  0.003 / 0.004  0.006 / 0.006  0.007 / 0.013  0.009 / 0.013  0.011 / 0.020  0.011 / 0.017  0.000 / 0.000
         5 cand  0.001 / 0.001  0.005 / 0.005  0.009 / 0.009  0.012 / 0.018  0.016 / 0.020  0.020 / 0.040  0.018 / 0.025  0.000 / 0.000
         9 cand  0.001 / 0.001  0.009 / 0.009  0.017 / 0.019  0.022 / 0.034  0.027 / 0.042  0.035 / 0.045  0.034 / 0.046  0.000 / 0.000

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.47            0.47            0.47
         2          5.10            4.63            2.55
         4         26.79           12.90            6.70
         6         65.19           21.25           10.86
         8        120.26           29.61           15.03
        10        196.16           38.31           19.62
        12        282.36           46.52           23.53
        15        291.27           52.88           19.42

slowest keystroke 52.88ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.
```

## Reproduction

在干净基线检出中应用下列冻结补丁，仅用于复现实验，不接入生产代码。模型由上述 SHA-256 校验，主项目评测集固定 `60ae46ff73afeecbfe1efa2a367cd06ae554d715` 的 `resources/eval/sentences-nine-key-v1.tsv`。不需要新增依赖或 unsafe。编译、正确性和计时分开：

```sh
cargo test --manifest-path reference/Cargo.toml --locked
MSIME_NEURAL_MODEL_DIR=<固定模型目录> cargo test --manifest-path reference/Cargo.toml --release --locked real_model_forward_matches_legacy_bits -- --ignored
cargo +1.75.0 test --manifest-path reference/Cargo.toml --locked
cargo clippy --manifest-path reference/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path reference/Cargo.toml --release --locked --no-run
# 构建终态后直接运行测试二进制，避免把 cargo 或编译启动算进计时。
reference/target/release/deps/chinese_ime_lm-<测试哈希> --ignored --exact attention_scratch_tests::six_layer_attention_paired_budget --nocapture
cargo build --manifest-path reference/Cargo.toml --release --locked --example bench
# 构建终态后保存并分别直接运行基线与实验二进制，逐轮监测并行构建。
reference/target/release/examples/bench <固定模型> <固定九键合成集>
```

```diff
diff --git a/reference/src/attention_scratch_tests.rs b/reference/src/attention_scratch_tests.rs
new file mode 100644
index 0000000..afe2b8f
--- /dev/null
+++ b/reference/src/attention_scratch_tests.rs
@@ -0,0 +1,213 @@
+use super::*;
+
+pub(super) fn reference_attention(
+    model: &SentenceModel,
+    q: &[f32],
+    k: &[f32],
+    v: &[f32],
+    rows: usize,
+    offset: usize,
+) -> Vec<f32> {
+    let n_embd = model.config.n_embd;
+    let heads = model.config.n_head;
+    let width = n_embd / heads;
+    let scale = 1.0 / (width as f32).sqrt();
+    let total = k.len() / n_embd;
+    let mut out = vec![0.0; rows * n_embd];
+    let mut scores = vec![0.0; total];
+    for row in 0..rows {
+        // 查询只能看到绝对位置 offset + row 及之前的位置。
+        let visible = offset + row + 1;
+        for head in 0..heads {
+            let base = head * width;
+            let query = &q[row * n_embd + base..row * n_embd + base + width];
+            let mut highest = f32::NEG_INFINITY;
+            for (step, score) in scores[..visible].iter_mut().enumerate() {
+                let key = &k[step * n_embd + base..step * n_embd + base + width];
+                *score = dot(query, key) * scale;
+                highest = highest.max(*score);
+            }
+            let mut sum = 0.0;
+            for score in &mut scores[..visible] {
+                *score = (*score - highest).exp();
+                sum += *score;
+            }
+            let target = &mut out[row * n_embd + base..row * n_embd + base + width];
+            for (step, &score) in scores[..visible].iter().enumerate() {
+                let weight = score / sum;
+                let value = &v[step * n_embd + base..step * n_embd + base + width];
+                for (slot, &item) in target.iter_mut().zip(value) {
+                    *slot += weight * item;
+                }
+            }
+        }
+    }
+    out
+}
+
+fn values(width: usize, rows: usize) -> Vec<f32> {
+    (0..width * rows)
+        .map(|i| {
+            if i % 11 == 0 {
+                -0.0
+            } else {
+                ((i * 7 % 31) as f32 - 15.0) / 16.0
+            }
+        })
+        .collect()
+}
+
+fn bits(values: &[f32]) -> Vec<u32> {
+    values.iter().map(|v| v.to_bits()).collect()
+}
+
+#[test]
+fn warmed_attention_keeps_output_and_scores() {
+    let model = super::feed_forward_scratch_tests::model(8, 1, true);
+    let mut scratch = AttentionScratch::default();
+    let q = values(8, 4);
+    let kv = values(8, 7);
+    model.attention_into(&q, &kv, &kv, 4, 3, &mut scratch);
+    let pointers = (scratch.output.as_ptr(), scratch.scores.as_ptr());
+    let capacities = (scratch.output.capacity(), scratch.scores.capacity());
+    model.attention_into(&q, &kv, &kv, 4, 3, &mut scratch);
+    assert_eq!(
+        pointers,
+        (scratch.output.as_ptr(), scratch.scores.as_ptr()),
+        "同形状 attention 仍替换已有缓冲"
+    );
+    assert_eq!(
+        capacities,
+        (scratch.output.capacity(), scratch.scores.capacity())
+    );
+}
+
+#[test]
+fn attention_reuse_matches_legacy_bits_and_clears_stale_output() {
+    for (width, heads) in [(1, 1), (7, 1), (8, 2), (9, 3), (192, 3), (768, 12)] {
+        let mut model = super::feed_forward_scratch_tests::model(width, 1, true);
+        model.config.n_head = heads;
+        let mut scratch = AttentionScratch::default();
+        for rows in [0, 1, 4, 12, 4, 1, 0] {
+            for offset in [0, 3, 31] {
+                let q = values(width, rows);
+                let k = values(width, rows + offset);
+                let mut v = k.clone();
+                v.reverse();
+                let expected = reference_attention(&model, &q, &k, &v, rows, offset);
+                scratch.output.fill(f32::NAN);
+                scratch.scores.fill(f32::NAN);
+                model.attention_into(&q, &k, &v, rows, offset, &mut scratch);
+                assert_eq!(
+                    bits(&expected),
+                    bits(&scratch.output),
+                    "width={width} heads={heads} rows={rows} offset={offset}"
+                );
+                assert_eq!(scratch.output.len(), rows * width);
+                assert_eq!(scratch.scores.len(), rows + offset);
+            }
+        }
+    }
+}
+
+#[inline(never)]
+fn old_chain(model: &SentenceModel, q: &[f32], kv: &[f32], rows: usize, offset: usize) -> Vec<f32> {
+    let mut x = q.to_vec();
+    for _ in 0..6 {
+        let attended = reference_attention(model, &x, kv, kv, rows, offset);
+        for (slot, value) in x.iter_mut().zip(attended) {
+            *slot += value;
+        }
+    }
+    x
+}
+
+#[inline(never)]
+fn new_chain(model: &SentenceModel, q: &[f32], kv: &[f32], rows: usize, offset: usize) -> Vec<f32> {
+    let mut x = q.to_vec();
+    let mut scratch = AttentionScratch::default();
+    for _ in 0..6 {
+        model.attention_into(&x, kv, kv, rows, offset, &mut scratch);
+        for (slot, &value) in x.iter_mut().zip(&scratch.output) {
+            *slot += value;
+        }
+    }
+    x
+}
+
+type Chain = fn(&SentenceModel, &[f32], &[f32], usize, usize) -> Vec<f32>;
+fn elapsed(
+    chain: Chain,
+    model: &SentenceModel,
+    q: &[f32],
+    kv: &[f32],
+    rows: usize,
+    offset: usize,
+    repeats: usize,
+) -> f64 {
+    use std::hint::black_box;
+    let start = std::time::Instant::now();
+    for _ in 0..repeats {
+        drop(black_box(chain(
+            black_box(model),
+            black_box(q),
+            black_box(kv),
+            rows,
+            offset,
+        )));
+    }
+    start.elapsed().as_secs_f64()
+}
+
+#[test]
+#[ignore = "独占运行完整六层 attention 配对预算，包含工作区构造析构"]
+fn six_layer_attention_paired_budget() {
+    let mut failures = Vec::new();
+    for width in [192, 768] {
+        let mut model = super::feed_forward_scratch_tests::model(width, 1, true);
+        model.config.n_head = if width == 192 { 3 } else { 12 };
+        for rows in [1, 4, 12] {
+            for offset in [0, 19] {
+                let q = values(width, rows);
+                let kv = values(width, rows + offset);
+                assert_eq!(
+                    bits(&old_chain(&model, &q, &kv, rows, offset)),
+                    bits(&new_chain(&model, &q, &kv, rows, offset))
+                );
+                let mut repeats = 1;
+                while elapsed(old_chain, &model, &q, &kv, rows, offset, repeats) < 0.010 {
+                    repeats *= 2;
+                }
+                let mut ratios = Vec::new();
+                for pair in 0..40 {
+                    let (old, new) = if pair % 2 == 0 {
+                        let old = elapsed(old_chain, &model, &q, &kv, rows, offset, repeats);
+                        (
+                            old,
+                            elapsed(new_chain, &model, &q, &kv, rows, offset, repeats),
+                        )
+                    } else {
+                        let new = elapsed(new_chain, &model, &q, &kv, rows, offset, repeats);
+                        (
+                            elapsed(old_chain, &model, &q, &kv, rows, offset, repeats),
+                            new,
+                        )
+                    };
+                    ratios.push(new / old);
+                }
+                ratios.sort_by(f64::total_cmp);
+                let ratio = ratios[19];
+                let budget = if width == 192 && rows == 1 {
+                    0.99
+                } else {
+                    1.03
+                };
+                println!("width={width} rows={rows} offset={offset} repeats={repeats} paired_ratio_p50={ratio:.4} budget={budget:.2}");
+                if ratio >= budget {
+                    failures.push((width, rows, offset, ratio, budget));
+                }
+            }
+        }
+    }
+    assert!(failures.is_empty(), "完整 attention 预算失败：{failures:?}");
+}
diff --git a/reference/src/feed_forward_scratch_tests.rs b/reference/src/feed_forward_scratch_tests.rs
index a5d0e16..d98ebb0 100644
--- a/reference/src/feed_forward_scratch_tests.rs
+++ b/reference/src/feed_forward_scratch_tests.rs
@@ -79,7 +79,8 @@ fn reference_run_prefix(model: &SentenceModel, tokens: &[u32]) -> Prefix<'static
             3 * n_embd,
         );
         let (q, k, v) = split_qkv(&qkv, tokens.len(), n_embd, &[], &[]);
-        let attended = model.attention(&q, &k, &v, tokens.len(), 0);
+        let attended =
+            super::attention_scratch_tests::reference_attention(model, &q, &k, &v, tokens.len(), 0);
         let projected = reference_linear(
             &attended,
             &block.proj_weight,
@@ -151,7 +152,14 @@ fn reference_run_tail_states(
             &prefix_keys[layer][..prefix_width],
             &prefix_values[layer][..prefix_width],
         );
-        let attended = model.attention(&q, &keys, &values, tokens.len(), length);
+        let attended = super::attention_scratch_tests::reference_attention(
+            model,
+            &q,
+            &keys,
+            &values,
+            tokens.len(),
+            length,
+        );
         let projected = reference_linear(
             &attended,
             &block.proj_weight,
@@ -196,7 +204,7 @@ fn matrix(rows: usize, width: usize, quantized: bool, salt: usize) -> Matrix {
     }
 }

-fn model(width: usize, layers: usize, quantized: bool) -> SentenceModel {
+pub(super) fn model(width: usize, layers: usize, quantized: bool) -> SentenceModel {
     let vector = |count, salt| {
         (0..count)
             .map(|i| (((i * 3 + salt) % 19) as f32 - 9.0) / 32.0)
diff --git a/reference/src/lib.rs b/reference/src/lib.rs
index e791c7b..2b0350e 100644
--- a/reference/src/lib.rs
+++ b/reference/src/lib.rs
@@ -184,6 +184,12 @@ struct Block {
     out_bias: Vec<f32>,
 }

+#[derive(Default)]
+struct AttentionScratch {
+    output: Vec<f32>,
+    scores: Vec<f32>,
+}
+
 #[derive(Default)]
 struct FeedForwardScratch {
     normalized: Vec<f32>,
@@ -635,6 +641,7 @@ impl SentenceModel {
         let n_embd = self.config.n_embd;
         let mut x = self.embed(tokens, 0);
         let mut feed_forward = FeedForwardScratch::default();
+        let mut attention = AttentionScratch::default();
         let mut keys = Vec::with_capacity(self.blocks.len());
         let mut values = Vec::with_capacity(self.blocks.len());
         for block in &self.blocks {
@@ -647,9 +654,9 @@ impl SentenceModel {
                 3 * n_embd,
             );
             let (q, k, v) = split_qkv(&qkv, tokens.len(), n_embd, &[], &[]);
-            let attended = self.attention(&q, &k, &v, tokens.len(), 0);
+            self.attention_into(&q, &k, &v, tokens.len(), 0, &mut attention);
             let projected = linear(
-                &attended,
+                &attention.output,
                 &block.proj_weight,
                 &block.proj_bias,
                 n_embd,
@@ -737,6 +744,7 @@ impl SentenceModel {
         let n_embd = self.config.n_embd;
         let mut x = self.embed(tokens, length);
         let mut feed_forward = FeedForwardScratch::default();
+        let mut attention = AttentionScratch::default();
         let mut keys_out = Vec::with_capacity(self.blocks.len());
         let mut values_out = Vec::with_capacity(self.blocks.len());
         for (layer, block) in self.blocks.iter().enumerate() {
@@ -755,9 +763,9 @@ impl SentenceModel {
                 &prefix_keys[layer][..prefix_width],
                 &prefix_values[layer][..prefix_width],
             );
-            let attended = self.attention(&q, &keys, &values, tokens.len(), length);
+            self.attention_into(&q, &keys, &values, tokens.len(), length, &mut attention);
             let projected = linear(
-                &attended,
+                &attention.output,
                 &block.proj_weight,
                 &block.proj_bias,
                 n_embd,
@@ -889,16 +897,27 @@ impl SentenceModel {
         self.index.get(&character).copied()
     }

-    fn attention(&self, q: &[f32], k: &[f32], v: &[f32], rows: usize, offset: usize) -> Vec<f32> {
+    fn attention_into(
+        &self,
+        q: &[f32],
+        k: &[f32],
+        v: &[f32],
+        rows: usize,
+        offset: usize,
+        scratch: &mut AttentionScratch,
+    ) {
         let n_embd = self.config.n_embd;
         let heads = self.config.n_head;
         let width = n_embd / heads;
         let scale = 1.0 / (width as f32).sqrt();
         let total = k.len() / n_embd;
-        let mut out = vec![0.0; rows * n_embd];
-        let mut scores = vec![0.0; total];
+        scratch.output.resize(rows * n_embd, 0.0);
+        scratch.output.fill(0.0);
+        scratch.scores.resize(total, 0.0);
+        let out = &mut scratch.output;
+        let scores = &mut scratch.scores;
         for row in 0..rows {
-            // Causal: a query at absolute position `offset + row` may attend to itself and earlier.
+            // 查询只能看到绝对位置 offset + row 及之前的位置。
             let visible = offset + row + 1;
             for head in 0..heads {
                 let base = head * width;
@@ -924,7 +943,6 @@ impl SentenceModel {
                 }
             }
         }
-        out
     }
 }

@@ -1857,3 +1875,6 @@ mod tests {

 #[cfg(test)]
 mod feed_forward_scratch_tests;
+
+#[cfg(test)]
+mod attention_scratch_tests;
```
