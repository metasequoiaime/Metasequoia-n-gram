# Agent Note: 同批候选复用已经计算的前缀

Status: implemented

## Problem

`Reranker::score` 只搜索上一轮 `resume`，不搜索本轮已放入 `next` 的结果。冷批次的重复候选、共享前缀和依次延长仍重复计算同一因果位置及归一化，末位置按需缓存没有覆盖这条路径。

## Decision

完全命中先查上一轮，再查本轮已有结果，直接共享 `Arc<TailRun>`。部分命中在两个集合中选择最长共享前缀；等长时保留上一轮原有的最后一个匹配，避免无收益地更换续算切点。新运行仍在本轮结束时替换旧集合，不增加跨轮寿命；候选顺序、预算、上下文失效和公开 API 保持。

## Existing decisions

检索活跃笔记中的缓存、同批、前缀、累计和浮点：[完全命中](../../implemented/bug-fix/2026-10-15-identical-rerank-cache.md)、[共享归一化](../../implemented/bug-fix/2026-10-15-shared-state-normalization.md)、[末位置按需前向](../../implemented/bug-fix/2026-10-15-defer-terminal-forward.md)均部分重叠，本篇补充其复用来源。没有需要废弃的同批提案。

## Alternatives considered

- 建 token trie：能统一发现整个批次的共享前缀，并消除重复扫描，但引入新缓存结构及遍历次序；先复用现有最多九候选的状态，避免没有证据的结构改写。
- 只消除完全重复：实现和浮点风险最低，但真实引擎候选大多是不同尾字的同前缀，不能消除已定位的共同位置前向。
- 直接让本轮覆盖上一轮等长匹配：实现最短，但同样长度没有额外省前向的收益，却改变旧缓存的浮点分组来源。

## Verification

合成回归先在旧代码失败：四候选冷批次前向 11 个位置，预期 5；新实现前向 5、投影 4 次，重复候选共享完整运行。覆盖重排、缓存释放、上下文失效、等长匹配保留旧轮最后项、末位置共享补算。既有三精度序列覆盖增删、未知 token 和窗口截断。Rust 1.97.1 与 1.75.0 各 32 项测试通过，Send/Sync 保持。

固定 msime int8 模型，SHA256 `86ac529510cb3b4968a5e6ade83ec8080f5b34a0a681e75c362accbbd387d1c1`。对照上游固定 `4c213a872c2afecf20b49aca8131e30537c39b55`，141 条全拼候选和 81 条九键候选，各取前九个，经缩短、恢复、补三个重复项、倒序、恢复五轮。1110 轮、10646 个评分中 1014 个位模式不同；全拼最大绝对误差 `9.536743e-7`，九键 `1.4305115e-6`，小于预设 `2e-6`；冷评分与跨轮评分的首选均相同。前九条离线对照的全拼解码器桶为 104/131、九键 43/74，词典桶分别 9/10、7/7。官方 `replay` 从完整列表先过滤可比项再截九条，另核对全拼词典 9/9、解码器 104/131，九键词典 7/7、解码器 42/74；旧新一致。两种截取顺序的池不同，不能混用数字，也不把只有一个可比候选的词典条目算入官方桶。

## Measurements

完整 bench：`bench <固定模型> eval/sentences-v1.tsv`，每形状 40 次。它的单次决策表调用不持有会话的 `SentenceModel::score`，本改动没有优化该入口；不能把这张冷表的波动归因于本改动。顺序整表有明显调度尖峰，收益判断采用同进程交替配对。长句候选不足时按工具原规则重复，保留原提示。

### 旧实现完整表

```text
load 3.680666ms for 4.5 MB
attribution: Trained on the Chinese portion of C4 (ODC-BY, https://huggingface.co/datasets/allenai/c4) and LCCC (MIT, https://github.com/thu-coai/CDial-GPT).
60 sentences from eval/sentences-v1.tsv

one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.48 /  0.49   0.96 /  0.98   1.91 /  1.96   2.88 /  3.12   3.83 /  4.71   4.80 / 15.27   5.76 / 63.44   7.20 / 21.64
         3 cand   0.48 /  0.51   1.93 /  1.98   4.80 /  4.97   7.76 / 11.16  10.59 / 11.06  13.42 / 13.54  16.38 / 16.56  20.71 / 21.47
         5 cand   0.49 /  0.50   2.91 /  3.13   7.68 /  7.88  12.47 / 12.65  17.15 / 17.45  21.49 / 22.11  26.25 / 26.96  33.17 / 33.82
         9 cand   0.49 /  0.51   4.72 /  4.89  13.06 / 13.46  21.47 / 21.97  29.99 / 30.89  38.27 / 38.87  47.14 / 47.60  60.41 / 64.38
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
         1 cand  0.001 / 0.001  0.002 / 0.003  0.003 / 0.003  0.004 / 0.005  0.006 / 0.011  0.008 / 0.018  0.009 / 0.012  0.011 / 0.018
         3 cand  0.002 / 0.003  0.005 / 0.006  0.009 / 0.009  0.015 / 0.017  0.022 / 0.026  0.027 / 0.037  0.027 / 0.032  0.028 / 0.042
         5 cand  0.004 / 0.004  0.009 / 0.009  0.015 / 0.016  0.026 / 0.028  0.036 / 0.040  0.046 / 0.051  0.042 / 0.049  0.056 / 0.063
         9 cand  0.008 / 0.008  0.016 / 0.016  0.028 / 0.051  0.042 / 0.048  0.065 / 0.074  0.083 / 0.098  0.071 / 0.081  0.096 / 0.157

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.45            0.45            0.45
         2          5.20            4.75            2.60
         4         27.45           13.19            6.86
         6         66.78           21.74           11.13
         8        122.56           29.89           15.32
        10        194.82           38.46           19.48
        12        297.23           48.26           24.77
        15        527.13           90.93           35.14

slowest keystroke 90.93ms. Pass --budget-ms to fail when it is too slow;
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
load 7.870167ms for 4.5 MB
attribution: Trained on the Chinese portion of C4 (ODC-BY, https://huggingface.co/datasets/allenai/c4) and LCCC (MIT, https://github.com/thu-coai/CDial-GPT).
60 sentences from eval/sentences-v1.tsv

one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.48 /  1.42   0.97 /  0.98   1.92 /  6.56   2.89 / 13.92   3.87 / 19.47   4.81 / 13.40   5.76 / 15.57   7.27 / 31.32
         3 cand   0.49 /  4.17   1.93 /  7.99   4.83 / 21.01   7.76 / 31.48  10.55 / 24.12  13.53 / 44.27  16.36 / 76.63  24.85 /117.62
         5 cand   0.49 /  0.49   2.91 / 17.43   7.73 / 78.14  12.51 / 94.56  24.59 / 92.41  22.10 /134.10  29.82 /103.25  41.56 / 83.29
         9 cand   0.49 /  0.50   5.12 / 48.37  26.73 / 54.68  26.52 / 88.63  42.33 / 99.53  48.70 / 90.54  54.25 /101.46  61.52 / 70.44
some cells repeated candidates: the pool has too few sentences that long. Cost depends
on the shape of the request, so the timings hold, but the text in those cells is not 9 distinct
sentences.

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.001  0.000 / 0.000  0.000 / 0.000  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001
         5 cand  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.002 / 0.002  0.003 / 0.003  0.005 / 0.006  0.007 / 0.020  0.010 / 0.019  0.009 / 0.013  0.011 / 0.020
         3 cand  0.001 / 0.001  0.005 / 0.006  0.009 / 0.009  0.016 / 0.018  0.023 / 0.030  0.031 / 0.034  0.028 / 0.033  0.024 / 0.028
         5 cand  0.001 / 0.001  0.007 / 0.008  0.016 / 0.016  0.027 / 0.031  0.039 / 0.042  0.052 / 0.056  0.050 / 0.054  0.023 / 0.026
         9 cand  0.002 / 0.002  0.008 / 0.008  0.029 / 0.030  0.050 / 0.055  0.071 / 0.092  0.081 / 0.083  0.050 / 0.056  0.026 / 0.129

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.49            0.49            0.49
         2          5.33            4.84            2.66
         4         27.84           13.41            6.96
         6         66.30           21.40           11.05
         8        146.63           39.66           18.33
        10        241.93           50.09           24.19
        12        284.97           47.04           23.75
        15        468.94           61.57           31.26

slowest keystroke 61.57ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.

ranking check:
   -2.6724  我今天去上班
   -4.1121  我今天去上版
   -4.9305  握今天去上班
   -5.0580  我今天趣上班
   -7.6756  握紧天趣上班
```

### 同进程交替顺序配对

旧、新代码使用相同模块编译方式，固定同一权重；每形状丢弃 4 对预热，保留 40 对，交替 old/new 与 new/old。`independent` 的各候选首 token 互异；`shared` 只有末 token 不同，且各末 token 互异；`pool` 使用公开评测候选，部分含重复；`unchanged` 是该评测池的热缓存。预设共享多候选至少改善 5%，独立候选不得出现超过 3% 的稳定中位回退。独立形状配对中位比 `0.9963–1.0038`；共享多候选全部通过，不能把池中重复项的较大收益推广给独立候选。热表只有微秒级，计时和共享状态局部性使比值放大，不单独用其比值承诺产品收益。共享配对末段期间有一次下游 CMake 配置（约 3 秒），没有并行完整构建；中位数及独立形状对照均保留。

```text
independent chars= 2 count=1 old_ms=0.937042 new_ms=0.935542 paired_ratio=0.9997
independent chars= 2 count=3 old_ms=1.898375 new_ms=1.909625 paired_ratio=1.0038
independent chars= 2 count=5 old_ms=2.869208 new_ms=2.870208 paired_ratio=0.9980
independent chars= 2 count=9 old_ms=4.764708 new_ms=4.742125 paired_ratio=0.9963
independent chars= 4 count=1 old_ms=1.893959 new_ms=1.889792 paired_ratio=0.9972
independent chars= 4 count=3 old_ms=4.764791 new_ms=4.758792 paired_ratio=0.9991
independent chars= 4 count=5 old_ms=7.567417 new_ms=7.549958 paired_ratio=1.0021
independent chars= 4 count=9 old_ms=13.277250 new_ms=13.243958 paired_ratio=0.9978
independent chars= 8 count=1 old_ms=3.798250 new_ms=3.770458 paired_ratio=0.9985
independent chars= 8 count=3 old_ms=10.390042 new_ms=10.398792 paired_ratio=1.0008
independent chars= 8 count=5 old_ms=16.948667 new_ms=17.004625 paired_ratio=1.0027
independent chars= 8 count=9 old_ms=30.186208 new_ms=30.291375 paired_ratio=0.9994
independent chars=15 count=1 old_ms=7.055375 new_ms=7.014417 paired_ratio=0.9995
independent chars=15 count=3 old_ms=20.050000 new_ms=20.030833 paired_ratio=0.9991
independent chars=15 count=5 old_ms=32.273584 new_ms=32.266458 paired_ratio=0.9991
independent chars=15 count=9 old_ms=59.795500 new_ms=59.901875 paired_ratio=1.0027
shared chars= 2 count=1 old_ms=0.941042 new_ms=0.951292 paired_ratio=1.0002
shared chars= 2 count=3 old_ms=1.909417 new_ms=0.958875 paired_ratio=0.5016
shared chars= 2 count=5 old_ms=2.875416 new_ms=0.961625 paired_ratio=0.3351
shared chars= 2 count=9 old_ms=4.753500 new_ms=0.962833 paired_ratio=0.2024
shared chars= 4 count=1 old_ms=1.907792 new_ms=1.903208 paired_ratio=0.9978
shared chars= 4 count=3 old_ms=4.750500 new_ms=1.906875 paired_ratio=0.4007
shared chars= 4 count=5 old_ms=7.609625 new_ms=1.912791 paired_ratio=0.2509
shared chars= 4 count=9 old_ms=13.270666 new_ms=1.925000 paired_ratio=0.1446
shared chars= 8 count=1 old_ms=3.779125 new_ms=3.741084 paired_ratio=0.9958
shared chars= 8 count=3 old_ms=10.423792 new_ms=3.817917 paired_ratio=0.3655
shared chars= 8 count=5 old_ms=17.055042 new_ms=3.817917 paired_ratio=0.2232
shared chars= 8 count=9 old_ms=29.951666 new_ms=3.874375 paired_ratio=0.1287
shared chars=15 count=1 old_ms=7.066583 new_ms=7.063583 paired_ratio=1.0015
shared chars=15 count=3 old_ms=20.191916 new_ms=7.094125 paired_ratio=0.3491
shared chars=15 count=5 old_ms=33.431375 new_ms=7.171334 paired_ratio=0.2131
shared chars=15 count=9 old_ms=59.761084 new_ms=7.206875 paired_ratio=0.1201
pool chars= 2 count=1 old_ms=0.955875 new_ms=0.955458 paired_ratio=0.9983
pool chars= 2 count=3 old_ms=1.924375 new_ms=1.919292 paired_ratio=0.9977
pool chars= 2 count=5 old_ms=2.876709 new_ms=2.397834 paired_ratio=0.8313
pool chars= 2 count=9 old_ms=4.778209 new_ms=2.407625 paired_ratio=0.5043
pool chars= 4 count=1 old_ms=1.903333 new_ms=1.899833 paired_ratio=0.9985
pool chars= 4 count=3 old_ms=4.763375 new_ms=4.701958 paired_ratio=0.9932
pool chars= 4 count=5 old_ms=7.477250 new_ms=7.008417 paired_ratio=0.9353
pool chars= 4 count=9 old_ms=13.122958 new_ms=10.890417 paired_ratio=0.8237
pool chars= 8 count=1 old_ms=3.751542 new_ms=3.767792 paired_ratio=0.9965
pool chars= 8 count=3 old_ms=10.309166 new_ms=9.983167 paired_ratio=0.9700
pool chars= 8 count=5 old_ms=16.845583 new_ms=15.385542 paired_ratio=0.9202
pool chars= 8 count=9 old_ms=29.705208 new_ms=25.202458 paired_ratio=0.8534
pool chars=15 count=1 old_ms=6.952333 new_ms=6.957125 paired_ratio=0.9959
pool chars=15 count=3 old_ms=20.141791 new_ms=13.574041 paired_ratio=0.6747
pool chars=15 count=5 old_ms=33.073584 new_ms=13.466125 paired_ratio=0.4078
pool chars=15 count=9 old_ms=59.442667 new_ms=13.543958 paired_ratio=0.2284
unchanged chars= 2 count=1 old_ms=0.000250 new_ms=0.000208 paired_ratio=1.0000
unchanged chars= 2 count=3 old_ms=0.000917 new_ms=0.000583 paired_ratio=0.6667
unchanged chars= 2 count=5 old_ms=0.001208 new_ms=0.000875 paired_ratio=0.8007
unchanged chars= 2 count=9 old_ms=0.002083 new_ms=0.001250 paired_ratio=0.6436
unchanged chars= 4 count=1 old_ms=0.000541 new_ms=0.000334 paired_ratio=0.7293
unchanged chars= 4 count=3 old_ms=0.001291 new_ms=0.000917 paired_ratio=0.6903
unchanged chars= 4 count=5 old_ms=0.001875 new_ms=0.001208 paired_ratio=0.6819
unchanged chars= 4 count=9 old_ms=0.003166 new_ms=0.002375 paired_ratio=0.7418
unchanged chars= 8 count=1 old_ms=0.000959 new_ms=0.001000 paired_ratio=0.9160
unchanged chars= 8 count=3 old_ms=0.002417 new_ms=0.002042 paired_ratio=0.7399
unchanged chars= 8 count=5 old_ms=0.003667 new_ms=0.002625 paired_ratio=0.6628
unchanged chars= 8 count=9 old_ms=0.006708 new_ms=0.004083 paired_ratio=0.6191
unchanged chars=15 count=1 old_ms=0.001667 new_ms=0.001500 paired_ratio=0.7951
unchanged chars=15 count=3 old_ms=0.004125 new_ms=0.001583 paired_ratio=0.3767
unchanged chars=15 count=5 old_ms=0.006417 new_ms=0.001583 paired_ratio=0.2447
unchanged chars=15 count=9 old_ms=0.012125 new_ms=0.002542 paired_ratio=0.2179
```

## Consequences

每候选增加对本轮已算项的线性扫描，缓存条目数和寿命仍只受当前/上一轮入参约束；没有新依赖或全局缓存。更长共享切点改变 f32 分组，评分并非逐位不变，测量限定误差和排序风险。本轮只复用先出现的状态，不重排候选去追逐最大理论收益。主仓库接入需另跑质量、逐键预算和 quick；局部收益不代表九键已经满足 16 ms。
