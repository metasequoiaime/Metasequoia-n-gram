# Agent Note: 上文最后一层只计算末位置输出

Status: implemented

## Problem

`run_prefix` 返回各层完整 KV 和末位置隐藏状态；最后一层却仍为每个上文位置运行 attention 输出投影与前馈。该层之前位置的输出没有下一层，也不进入返回结果。真实九键采样仍以线性层和 int8 点积为主要模型热点；没有上文时仅 BOS 一行，没有可删除的额外行，不能预言此片会让九键达到 16 ms。

## Decision

保留所有层的完整归一化、QKV 和完整 KV。仅在最后一层，从末位置 Q 运行一行 attention，使用其绝对 offset 访问完整 KV，再只更新末位置残差及前馈。其它层继续计算所有位置；最终归一化仍取末位置。单位置与零层配置继续原来有效行为，内部非空 token 前提不变。

## Existing decisions

已检索 `prefix`、前缀、最后一层、前向与缓冲。[末位置按需前向](../../implemented/bug-fix/2026-10-15-defer-terminal-forward.md)部分相关，持有候选末 token 的延迟规则；本片只删除独立上文末层的无用输出。[前馈工作区](../../implemented/testing/2026-10-16-feed-forward-scratch.md)继续持有同次前向复用规则；最后一层缩成一行不会改变备用容量。[完整 KV](../../implemented/testing/2026-10-16-packed-tail-kv.md)继续约束缓存必须保存所有可见位置。本片不覆盖既有决定，无完全吸收或过时提案。

## Alternatives considered

- 删除最后一层较早位置的 KV：能再省计算，但后续候选的最后一层仍需对这些 KV 做注意力，不能删除。
- 从每一层只算末位置：少算最多，但后续层需要前一层所有位置的状态来生成 KV，不符合因果依赖。
- 保留旧路径：循环统一，但上文越长，最后一层浪费的线性层计算越多；用已有 attention 的行数和 offset 表达需求即可，无需新模型格式或 API。

## Verification

计数回归在旧实现真实失败：一层四位置上文计算了四个查询，只应计算一个。完整上文配对门禁在旧实现也真实失败：16 形状中的 12 个多位置目标均未达到 <0.98（10.38 秒），未调整预设门槛。

Float/Quantized × 5 种宽度/头数（1/1、7/1、8/2、9/3、192/4）× 0/1/3 层 × 1/4/12 位置，共 90 组，全部 KV、末隐藏状态、完整分布和单/多 token 尾部续跑逐位一致。测试中的 `baseline_prefix` 冻结固定 `527d2fa` 完整入口，底层 helper 在本片保持不变；独立审查用源文本比对确认除 `self` 改 `model` 外一致。原有真实模型前缀/尾部逐位测试另有保留，本轮真实模型配对也直接比较全部 KV、末隐藏状态、完整分布及续跑累计分。

Rust 1.97.1 与最低 1.75.0 各通过 37 项单测、15 项模型文件测试和 doc tests，5 项手动测试默认 ignored。全目标 Clippy `-D warnings`、Rustfmt 通过。独立只读审查确认末层 offset、完整 KV、前面层及零层/单位置路径正确，没有阻塞项；审查者未运行编译或计时。

## Paired measurements

完整上文构建计时包含嵌入、全部层、归一化、最终缓存构造与析构。宽 192、4 头、1/6 层、1/4/12/32 位置、Float/Quantized，共 16 形状；每形状 21 对，交换先后，旧臂校准至少 10 ms。多位置 new/old 中位必须 <0.98，单位置 <1.05。old_us / new_us 是各侧单次完整上文构建微秒中位，ratio 为逐对比值中位，不是两侧中位之比。

首次新版门禁全部通过，但每秒采样观察到 5 次并行构建进程，不能作为无构建重叠验收；下面同时保留该次和完整重测，不挑形状。

### 首次新版（受并行构建干扰）

```text
layers=1 rows=1 quantized=false old_us=42.586 new_us=42.365 ratio=0.994802 p10=0.978373 p90=1.006603 budget=1.05
layers=1 rows=4 quantized=false old_us=165.958 new_us=71.546 ratio=0.432043 p10=0.423978 p90=0.439733 budget=0.98
layers=1 rows=12 quantized=false old_us=514.660 new_us=155.040 ratio=0.300612 p10=0.293082 p90=0.308982 budget=0.98
layers=1 rows=32 quantized=false old_us=1361.943 new_us=351.052 ratio=0.256704 p10=0.255097 p90=0.261588 budget=0.98
layers=6 rows=1 quantized=false old_us=265.669 new_us=264.823 ratio=1.000733 p10=0.972505 p90=1.016495 budget=1.05
layers=6 rows=4 quantized=false old_us=1000.419 new_us=906.013 ratio=0.903205 p10=0.886209 p90=0.915229 budget=0.98
layers=6 rows=12 quantized=false old_us=2990.802 new_us=2592.302 ratio=0.875445 p10=0.846696 p90=0.892879 budget=0.98
layers=6 rows=32 quantized=false old_us=8007.563 new_us=7024.979 ratio=0.877307 p10=0.863649 p90=0.896486 budget=0.98
layers=1 rows=1 quantized=true old_us=48.473 new_us=48.357 ratio=0.999112 p10=0.969324 p90=1.014374 budget=1.05
layers=1 rows=4 quantized=true old_us=190.646 new_us=83.177 ratio=0.435916 p10=0.416422 p90=0.444209 budget=0.98
layers=1 rows=12 quantized=true old_us=573.913 new_us=178.087 ratio=0.309332 p10=0.305597 p90=0.315205 budget=0.98
layers=1 rows=32 quantized=true old_us=1565.625 new_us=412.630 ratio=0.263808 p10=0.253170 p90=0.275098 budget=0.98
layers=6 rows=1 quantized=true old_us=285.721 new_us=287.420 ratio=1.003235 p10=0.985503 p90=1.025080 budget=1.05
layers=6 rows=4 quantized=true old_us=1149.411 new_us=1043.396 ratio=0.903163 p10=0.892180 p90=0.922264 budget=0.98
layers=6 rows=12 quantized=true old_us=3466.979 new_us=3046.646 ratio=0.884991 p10=0.852863 p90=0.897387 budget=0.98
layers=6 rows=32 quantized=true old_us=9371.334 new_us=8216.271 ratio=0.872408 p10=0.851401 p90=0.901180 budget=0.98
```

### 完整重测（无构建重叠观察）

16 形状全部通过（8.91 秒），每秒监测 9 次，构建进程观察数 0。

```text
layers=1 rows=1 quantized=false old_us=41.894 new_us=41.641 ratio=0.990785 p10=0.978198 p90=1.006247 budget=1.05
layers=1 rows=4 quantized=false old_us=164.951 new_us=71.303 ratio=0.430742 p10=0.421853 p90=0.445834 budget=0.98
layers=1 rows=12 quantized=false old_us=491.712 new_us=150.023 ratio=0.301537 p10=0.291169 p90=0.308276 budget=0.98
layers=1 rows=32 quantized=false old_us=1345.672 new_us=348.130 ratio=0.260099 p10=0.253180 p90=0.265290 budget=0.98
layers=6 rows=1 quantized=false old_us=258.339 new_us=258.477 ratio=1.000161 p10=0.988895 p90=1.009414 budget=1.05
layers=6 rows=4 quantized=false old_us=994.799 new_us=901.125 ratio=0.903669 p10=0.895686 p90=0.910329 budget=0.98
layers=6 rows=12 quantized=false old_us=2984.427 new_us=2637.000 ratio=0.883587 p10=0.872039 p90=0.896393 budget=0.98
layers=6 rows=32 quantized=false old_us=8055.521 new_us=7048.479 ratio=0.875859 p10=0.854560 p90=0.892307 budget=0.98
layers=1 rows=1 quantized=true old_us=47.914 new_us=48.924 ratio=1.009930 p10=0.992630 p90=1.036972 budget=1.05
layers=1 rows=4 quantized=true old_us=192.658 new_us=84.467 ratio=0.436152 p10=0.430397 p90=0.444730 budget=0.98
layers=1 rows=12 quantized=true old_us=572.745 new_us=175.419 ratio=0.306620 p10=0.293201 p90=0.313010 budget=0.98
layers=1 rows=32 quantized=true old_us=1558.448 new_us=409.120 ratio=0.261368 p10=0.252204 p90=0.266177 budget=0.98
layers=6 rows=1 quantized=true old_us=287.725 new_us=287.182 ratio=0.999770 p10=0.982446 p90=1.019863 budget=1.05
layers=6 rows=4 quantized=true old_us=1155.365 new_us=1045.878 ratio=0.904680 p10=0.877495 p90=0.928549 budget=0.98
layers=6 rows=12 quantized=true old_us=3476.927 new_us=3055.917 ratio=0.882926 p10=0.864477 p90=0.912925 budget=0.98
layers=6 rows=32 quantized=true old_us=9343.375 new_us=8136.521 ratio=0.880102 p10=0.862293 p90=0.896640 budget=0.98
```

### 固定真实模型

模型 SHA-256 `86ac529510cb3b4968a5e6ade83ec8080f5b34a0a681e75c362accbbd387d1c1`，8192 词表、6 层、4 头、宽 192、上下文 64。相同完整上文门槛和配对方法，5 形状全部通过（3.81 秒），每秒监测 4 次，构建进程观察数 0。全部 KV 和末隐藏状态逐位相同；完整分布相同；不足 64 位置时续跑三 token 的累计分也相同。

```text
layers=6 rows=1 quantized=true old_us=288.305 new_us=286.748 ratio=0.999942 p10=0.978609 p90=1.019852 budget=1.05
layers=6 rows=4 quantized=true old_us=1134.979 new_us=1039.398 ratio=0.907944 p10=0.892714 p90=0.931127 budget=0.98
layers=6 rows=12 quantized=true old_us=3463.166 new_us=3031.438 ratio=0.878806 p10=0.852955 p90=0.903447 budget=0.98
layers=6 rows=32 quantized=true old_us=9293.438 new_us=8191.959 ratio=0.882948 p10=0.864446 p90=0.899903 budget=0.98
layers=6 rows=64 quantized=true old_us=19031.208 new_us=16673.791 ratio=0.875973 p10=0.848862 p90=0.886072 budget=0.98
```

## Consequences

六层真实模型 4–64 位置完整上文构建约省 9%–12%，单位置基本持平。收益来自删除没有消费者的末层输出；QKV、权重格式、公开 API、缓存寿命、数值运算顺序均保持。两份前馈备用缓冲的容量仍由前面层决定，不能声称峰值内存随末层行数同步下降。

最后一层绝对 offset 必须是上文末位置，否则 attention 会遗漏早期 KV；所有 KV 和此前层输出都必须保留。无上文时只有 BOS 一行，主要九键预算不会由此自动解决，主项目需固定合并提交并独立验收。每秒监测仅能排除观察到的构建重叠，不能证明机器没有其它负载。

## Complete model tables

固定相同真实模型、81 例九键句集，串行运行 `reference/examples/bench.rs` 全部表，每形状 40 次。旧二进制 SHA-256 `02f1630064e4c58e8f1f85d7143f8a56b3808e8bb8ab779f721a0054cde47980`，与已核对的 `6ce2cbb5` 构建一致；`6ce2cbb5..527d2fa` 的 reference 源码没有差异。新版 SHA-256 `0ceb3ee021d2ecc3696be9d2ca37b4316d0a417cf8ea9e425d7afab3f739f380`。

旧轮退出 0，每秒监测 51 次，构建进程观察数 0；新版首轮退出 0，55 次监测中有 6 次构建进程观察。不能用这组非配对且受干扰的整表推断稳定整体收益或回退；确定收益依据仍是上节完整上文配对门禁。所有尖峰保留，十五字列没有样本，0 值无效；逐句十五字行也不是实际十五字决策。

### 旧版完整表

```text
one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.47 /  0.47   0.90 /  0.93   1.81 /  1.87   2.71 /  2.79   3.61 /  3.64   4.52 /  4.57   5.42 /  5.47   0.00 /  0.00
         3 cand   0.46 /  0.47   1.86 /  1.89   4.54 /  4.69   7.32 /  7.50   9.93 / 10.09  12.65 / 12.81  15.36 / 15.74   0.00 /  0.00
         5 cand   0.46 /  0.47   2.79 /  2.85   7.26 /  7.50  11.75 / 11.82  16.25 / 16.38  20.77 / 20.92  25.29 / 25.44   0.00 /  0.00
         9 cand   0.46 /  0.47   4.54 /  4.59  12.66 / 12.75  20.83 / 21.11  28.94 / 29.59  37.06 / 37.62  45.22 / 45.84   0.00 /  0.00

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         5 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.002  0.001 / 0.002  0.002 / 0.002  0.002 / 0.002  0.000 / 0.000

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.002  0.002 / 0.004  0.003 / 0.007  0.007 / 0.009  0.005 / 0.008  0.000 / 0.000
         3 cand  0.001 / 0.001  0.003 / 0.004  0.005 / 0.006  0.007 / 0.012  0.010 / 0.013  0.010 / 0.019  0.011 / 0.018  0.000 / 0.000
         5 cand  0.001 / 0.001  0.004 / 0.005  0.009 / 0.009  0.013 / 0.026  0.017 / 0.019  0.020 / 0.035  0.019 / 0.028  0.000 / 0.000
         9 cand  0.001 / 0.001  0.009 / 0.015  0.016 / 0.017  0.022 / 0.032  0.030 / 0.037  0.039 / 0.043  0.040 / 0.052  0.000 / 0.000

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.47            0.47            0.47
         2          5.07            4.60            2.53
         4         26.31           12.67            6.58
         6         63.84           20.80           10.64
         8        118.07           29.25           14.76
        10        189.41           37.32           18.94
        12        274.11           45.35           22.84
        15        274.82           45.17           18.32

slowest keystroke 45.35ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.
```

### 新版首轮（受构建干扰）

```text
one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.48 /  0.52   0.93 /  0.95   1.86 /  1.90   2.71 /  2.76   3.61 /  3.65   4.52 /  4.57   5.43 /  5.48   0.00 /  0.00
         3 cand   0.45 /  0.46   1.82 /  1.88   4.63 /  4.65   7.24 /  7.33   9.94 / 10.22  12.68 / 13.05  15.59 / 15.89   0.00 /  0.00
         5 cand   0.47 /  0.48   2.71 /  2.78   7.29 /  7.54  11.77 / 11.93  16.28 / 16.55  20.78 / 21.08  25.43 / 26.83   0.00 /  0.00
         9 cand   0.48 /  1.11   4.69 / 10.47  13.04 / 17.74  21.21 / 21.46  29.88 / 30.85  44.06 / 77.77  56.77 /107.19   0.00 /  0.00

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         5 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.000 / 0.000

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.003  0.002 / 0.004  0.003 / 0.004  0.007 / 0.013  0.005 / 0.007  0.000 / 0.000
         3 cand  0.001 / 0.001  0.003 / 0.004  0.006 / 0.006  0.008 / 0.022  0.010 / 0.014  0.012 / 0.018  0.015 / 0.021  0.000 / 0.000
         5 cand  0.001 / 0.001  0.005 / 0.005  0.009 / 0.010  0.014 / 0.021  0.018 / 0.020  0.023 / 0.027  0.022 / 0.168  0.000 / 0.000
         9 cand  0.001 / 0.001  0.009 / 0.009  0.017 / 0.019  0.026 / 0.033  0.030 / 0.057  0.044 / 0.055  0.041 / 0.044  0.000 / 0.000

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.48            0.48            0.48
         2          5.17            4.70            2.59
         4         28.94           14.90            7.23
         6         67.39           22.37           11.23
         8        120.01           29.28           15.00
        10        190.99           39.05           19.10
        12        298.66           46.39           24.89
        15        278.73           46.18           18.58

slowest keystroke 46.39ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.
```

### 新版完整重测（受构建干扰）

重测退出 0，57 次监测合计 155 次构建进程观察，整表不能作为无构建重叠验收；不继续挑轮次。两轮新版和旧版的 ranking check 打印值均一致。

```text
one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.47 /  0.49   0.93 /  0.94   1.86 /  1.91   2.83 /  4.57   3.75 / 22.44   4.68 /  9.42   5.63 / 32.76   0.00 /  0.00
         3 cand   0.47 /  1.54   1.88 /  4.20   4.68 / 29.23   7.51 / 29.77  10.45 / 41.01  13.16 / 65.55  19.22 / 66.64   0.00 /  0.00
         5 cand   0.48 /  2.51   2.84 /  3.91   7.51 /  8.64  12.88 / 25.89  16.90 / 68.73  30.75 / 79.36  26.48 / 91.84   0.00 /  0.00
         9 cand   0.47 /  0.49   4.71 / 36.55  15.30 / 50.32  21.47 / 71.88  55.13 / 88.77  38.21 / 98.74  46.48 / 49.32   0.00 /  0.00

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         5 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.002  0.002 / 0.002  0.002 / 0.002  0.000 / 0.000

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.003  0.002 / 0.003  0.003 / 0.005  0.007 / 0.010  0.005 / 0.008  0.000 / 0.000
         3 cand  0.001 / 0.001  0.003 / 0.004  0.005 / 0.006  0.008 / 0.017  0.009 / 0.013  0.012 / 0.026  0.013 / 0.018  0.000 / 0.000
         5 cand  0.001 / 0.001  0.005 / 0.005  0.009 / 0.010  0.012 / 0.021  0.016 / 0.018  0.019 / 0.024  0.017 / 0.022  0.000 / 0.000
         9 cand  0.001 / 0.001  0.009 / 0.009  0.016 / 0.016  0.021 / 0.029  0.030 / 0.038  0.034 / 0.073  0.035 / 0.042  0.000 / 0.000

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.47            0.47            0.47
         2          5.14            4.66            2.57
         4         26.80           12.86            6.70
         6         64.85           21.14           10.81
         8        118.83           28.99           14.85
        10        188.08           37.15           18.81
        12        274.53           45.29           22.88
        15        274.17           45.24           18.28

slowest keystroke 45.29ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.
```
