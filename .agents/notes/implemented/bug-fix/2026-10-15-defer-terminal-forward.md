# Agent Note: 候选末位置的 Transformer 前向按续跑需要计算

Status: implemented

## Problem

`run_tail` 为候选每个 token 跑 Transformer，但位置 i 的隐藏状态预测 i+1；末 token 的累计概率已有，末位置前向只为可能的后续延长服务。末字频繁变化时这部分工作直接丢弃。热点采样仍以量化矩阵点积为主，矩阵循环改写未稳定达预算，应先消除不需要的完整前向。

## Decision

当前评分只计算 tail 长度减一的位置，保存完整累计分与一个待处理末 token。`TailRun::resume` 在真正经过该末 token 时通过 `OnceLock` 补算一次，并共享末位置的隐藏状态及其归一化。末位置缓存仅持有各层新增的一行 KV，避免重复保存整段前缀。普通部分续跑、`joined`、完全命中和上一轮缓存寿命不变。单字评分不计算候选 Transformer 位置。

## Existing decisions

已检索 `run_tail`、`resume`、隐藏状态与末位置笔记。[完全命中缓存](../../implemented/bug-fix/2026-10-15-identical-rerank-cache.md)继续持有完整累计分及上一轮寿命；[共享归一化](../../implemented/bug-fix/2026-10-15-shared-state-normalization.md)继续持有每个隐藏状态的归一化缓存。本篇补充末位置按需计算，不翻转评分或缓存约束。[行分派负结果](../../rejected/bug-fix/2026-10-15-matrix-row-dispatch.md)与[行交错负结果](../../rejected/bug-fix/2026-10-15-matrix-row-batching.md)属于不同机制，无需改写。

## Alternatives considered

- 一直提前计算末位置：续跑代码最简单，延长时无需补算，但末字改变和候选消失时浪费一次完整前向。
- 直接丢掉末状态并把最大共享长度截为旧候选长度减一：实现最少，但多候选延长同一个旧候选时重复计算末位置及归一化，破坏已经验证的共享约束。
- 延迟缓存整个完整 Prefix：容易复用现有结构，但重复持有长前缀 KV；只保存新增的一行，按需要组装前缀。

## References

[Rust OnceLock 文档](https://doc.rust-lang.org/std/sync/struct.OnceLock.html)核对 `get_or_init` 的线程安全与兼容性；继续使用现有 Rust 1.75 能力。[nanoGPT](https://github.com/karpathy/nanoGPT/blob/master/model.py)的 `forward` 展示按输出需要选择投影位置的做法，仅参考按需计算原则，没有复制代码。具体因果索引以本仓库的 attention 与累计分实现为准。

## Verification

旧实现的前向计数回归失败：2 个上文/BOS 位置和 2 条两字候选共算 6 个位置，预期只需 4 个。修复后冷计算只算 4 个；改末字和缩短为 0，两个候选延长同一旧候选只补算 1 个，多轮 joined 后继续共享。另覆盖没有已前向位置的单字候选延长，既有词表投影计数不退化。

固定旧提交 `0f4b47755462a0d6bf4a68261c5e5e11a11eed50`，生产代码与主项目原 pin 相同。真实模型 SHA256 为 `86ac529510cb3b4968a5e6ade83ec8080f5b34a0a681e75c362accbbd387d1c1`。141 条全拼和 81 条九键记录经缩短、恢复、重复、倒序、恢复共 1110 轮、11976 个评分逐位一致，冷评分和完整公开分布也逐位一致。旧新门控首选相同；排除 1 条可比候选不足的全拼记录后，全拼词典桶 9/9、解码器桶 104/131，九键词典桶 7/7、解码器桶 43/74，均保持。

Rust 1.97.1 与 MSRV 1.75.0 各通过 30 项测试（16 单元、14 集成），覆盖 f32/f16/int8 的缓存序列、窗口和上下文失效；全目标 Clippy、fmt、笔记目录/格式/链接/封印与差异检查通过。临时固定旧实现的审计程序已删除，不进入分发源码。

## Measurements

同一进程加载固定旧实现与新实现，每形状 80 对，交替顺序。`final` 在相同长度候选的甲/乙末字间交替；`extend` 每对先各自预热相同前缀候选，再测延长一个已知 token 的决策，预热不计时。候选与 bench 一样来自公开合成短句，长句不足时重复；重复候选可以共享一次末位置补算，因此不得把重复形状的较大收益推广到不同候选。32 种末字变化比值 0.0030–0.0480；32 种延长比值 0.3483–1.0157。保存全部形状，不挑最快的一行。

```text
mode count chars old_p50_us new_p50_us paired_new_over_old_p50
final 1 1 288.708 0.875 0.0030
final 1 2 302.459 2.041 0.0069
final 1 4 308.875 3.792 0.0120
final 1 6 300.292 5.792 0.0195
final 1 8 310.666 7.458 0.0234
final 1 10 308.458 10.584 0.0357
final 1 12 308.792 8.958 0.0287
final 1 15 315.500 11.291 0.0364
final 3 1 883.917 2.708 0.0030
final 3 2 893.125 6.000 0.0067
final 3 4 883.375 10.000 0.0112
final 3 6 902.417 18.333 0.0203
final 3 8 933.209 26.250 0.0280
final 3 10 918.208 33.708 0.0365
final 3 12 927.542 33.833 0.0366
final 3 15 932.959 35.250 0.0379
final 5 1 1470.125 4.458 0.0030
final 5 2 1518.583 10.875 0.0071
final 5 4 1524.125 19.125 0.0126
final 5 6 1547.083 36.167 0.0234
final 5 8 1562.875 45.666 0.0292
final 5 10 1592.334 65.875 0.0403
final 5 12 1614.959 67.250 0.0420
final 5 15 1612.125 69.167 0.0436
final 9 1 2706.375 9.291 0.0035
final 9 2 2716.000 19.542 0.0073
final 9 4 2761.458 37.333 0.0136
final 9 6 2796.417 69.292 0.0248
final 9 8 2819.791 85.833 0.0307
final 9 10 2850.792 112.417 0.0399
final 9 12 2877.042 121.458 0.0426
final 9 15 2939.584 139.416 0.0480
extend 1 1 482.333 486.208 1.0006
extend 1 2 494.625 495.250 1.0013
extend 1 4 489.875 493.875 1.0068
extend 1 6 495.542 499.375 1.0088
extend 1 8 495.708 506.291 1.0096
extend 1 10 507.750 511.958 1.0055
extend 1 12 507.458 516.625 1.0136
extend 1 15 513.958 519.584 1.0157
extend 3 1 1457.500 1457.625 1.0009
extend 3 2 1472.458 1469.667 1.0010
extend 3 4 1451.708 1463.709 1.0052
extend 3 6 1529.541 1532.625 0.9991
extend 3 8 1524.084 1556.666 1.0126
extend 3 10 1505.083 1529.250 1.0068
extend 3 12 1542.042 1565.959 1.0119
extend 3 15 1383.458 1088.416 0.7817
extend 5 1 2463.334 2067.541 0.8610
extend 5 2 2829.917 2947.959 1.0006
extend 5 4 2618.417 2658.750 1.0080
extend 5 6 2569.208 2605.666 1.0083
extend 5 8 2555.167 2588.334 1.0097
extend 5 10 2570.083 2591.791 1.0040
extend 5 12 2418.208 2425.917 1.0012
extend 5 15 1939.667 1049.083 0.5409
extend 9 1 3349.792 1885.833 0.5649
extend 9 2 4339.750 4342.000 1.0088
extend 9 4 4450.791 4469.500 1.0034
extend 9 6 4501.250 4538.250 1.0100
extend 9 8 4546.000 4602.000 1.0116
extend 9 10 4399.209 4091.792 0.9308
extend 9 12 3634.042 2475.958 0.6797
extend 9 15 3208.417 1119.125 0.3483
```

完整 bench 命令为 `<旧或新 bench> <固定模型> eval/sentences-v1.tsv`，每形状 40 次。两臂顺序运行，没有同时运行本任务编译、测试或其它 benchmark；机器其它任务仍可能干扰 p95。`0.000` 是显示精度，不表示零耗时。

### 旧实现完整表

```text
load 3.57125ms for 4.5 MB
attribution: Trained on the Chinese portion of C4 (ODC-BY, https://huggingface.co/datasets/allenai/c4) and LCCC (MIT, https://github.com/thu-coai/CDial-GPT).
60 sentences from eval/sentences-v1.tsv

one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.76 /  0.78   1.22 /  1.24   2.15 /  2.21   3.08 /  3.12   4.01 /  4.06   4.94 /  5.22   5.89 /  6.03   7.37 /  7.51
         3 cand   1.35 /  1.36   2.74 /  2.79   5.53 /  5.60   8.31 /  8.39  11.09 / 11.21  13.93 / 14.35  16.75 / 17.02  21.10 / 21.45
         5 cand   1.92 /  1.97   4.25 /  4.35   8.89 /  8.99  13.51 / 13.63  18.16 / 18.28  22.79 / 23.13  27.57 / 28.16  34.62 / 35.25
         9 cand   3.19 /  3.31   7.48 /  7.84  15.92 / 16.49  24.17 / 25.03  32.45 / 33.32  40.96 / 41.75  49.70 / 50.83  62.41 / 63.49
some cells repeated candidates: the pool has too few sentences that long. Cost depends
on the shape of the request, so the timings hold, but the text in those cells is not 9 distinct
sentences.

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001
         5 cand  0.000 / 0.000  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.291 / 0.321  0.285 / 0.317  0.287 / 0.300  0.303 / 0.312  0.298 / 0.316  0.302 / 0.324  0.309 / 0.319  0.312 / 0.325
         3 cand  0.887 / 0.906  0.890 / 0.917  0.922 / 0.953  0.932 / 0.979  0.939 / 0.974  0.926 / 1.017  0.976 / 1.048  0.904 / 0.970
         5 cand  1.460 / 1.550  1.525 / 1.564  1.517 / 1.605  1.544 / 1.628  1.563 / 1.622  1.583 / 1.655  1.592 / 1.753  1.612 / 1.703
         9 cand  2.685 / 2.768  2.710 / 2.807  2.758 / 2.865  2.791 / 2.916  2.800 / 2.922  2.819 / 2.959  2.827 / 3.043  2.897 / 3.027

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          3.18            3.18            3.18
         2         10.54            7.43            5.27
         4         39.17           16.26            9.79
         6         82.42           24.05           13.74
         8        141.29           32.63           17.66
        10        234.00           44.20           23.40
        12        347.19           54.16           28.93
        15        603.47           90.58           40.23

slowest keystroke 90.58ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.

ranking check:
   -2.6724  我今天去上班
   -4.1121  我今天去上版
   -4.9305  握今天去上班
   -5.0580  我今天趣上班
   -7.6756  握紧天趣上班
```

### 新实现完整表

```text
load 3.197ms for 4.5 MB
attribution: Trained on the Chinese portion of C4 (ODC-BY, https://huggingface.co/datasets/allenai/c4) and LCCC (MIT, https://github.com/thu-coai/CDial-GPT).
60 sentences from eval/sentences-v1.tsv

one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.48 /  0.50   0.96 /  0.99   1.90 /  1.94   2.86 /  2.92   3.72 /  3.88   4.69 /  4.81   5.66 /  5.81   7.11 /  7.23
         3 cand   0.48 /  0.50   1.88 /  1.96   4.74 /  4.88   7.53 /  7.71  10.39 / 10.60  13.13 / 13.41  16.00 / 16.34  20.19 / 20.57
         5 cand   0.48 /  0.49   2.83 /  2.94   7.61 /  7.76  12.32 / 12.53  17.02 / 17.26  21.81 / 22.01  25.79 / 26.55  33.78 / 35.72
         9 cand   0.49 /  0.49   4.76 /  4.90  13.21 / 13.46  21.58 / 22.07  30.71 / 35.24  40.22 / 52.98  48.88 / 57.71  66.06 /119.83
some cells repeated candidates: the pool has too few sentences that long. Cost depends
on the shape of the request, so the timings hold, but the text in those cells is not 9 distinct
sentences.

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001
         5 cand  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.002 / 0.004  0.003 / 0.003  0.005 / 0.005  0.006 / 0.009  0.008 / 0.016  0.008 / 0.016  0.010 / 0.017
         3 cand  0.002 / 0.003  0.005 / 0.006  0.010 / 0.010  0.016 / 0.023  0.021 / 0.025  0.030 / 0.057  0.031 / 0.040  0.029 / 0.044
         5 cand  0.004 / 0.004  0.009 / 0.009  0.015 / 0.017  0.025 / 0.034  0.034 / 0.045  0.046 / 0.057  0.047 / 0.056  0.051 / 0.059
         9 cand  0.008 / 0.009  0.017 / 0.021  0.028 / 0.029  0.046 / 0.064  0.068 / 0.120  0.090 / 0.112  0.086 / 0.134  0.100 / 0.110

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.49            0.49            0.49
         2          5.30            4.81            2.65
         4         27.92           13.47            6.98
         6         67.98           22.10           11.33
         8        123.45           30.19           15.43
        10        197.84           38.78           19.78
        12        289.11           48.35           24.09
        15        455.81           60.23           30.39

slowest keystroke 60.23ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.

ranking check:
   -2.6724  我今天去上班
   -4.1121  我今天去上版
   -4.9305  握今天去上班
   -5.0580  我今天趣上班
   -7.6756  握紧天趣上班
```

## Consequences

评分单位、候选数量、门控、归一化和逐行点积保持。KV 长度比累计分少一个候选位置，resume 和 joined 经过末位置时补齐；位置分段保持既有累计分分组。每条运行增加待处理 token 和 OnceLock，真正延长时持有各层一行 KV 与隐藏状态，寿命仍受上一轮缓存约束；普通尾部不提前计算。延长时的临时 KV 克隆造成小额成本，需同时保留延长对照。主项目需单独固定新提交并验证质量、逐键预算与 quick，不能用局部收益声称已达九键 16 ms。
