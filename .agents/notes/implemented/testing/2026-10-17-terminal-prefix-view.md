# Agent Note: 末位置续跑借用两段 KV

Status: implemented

## Problem

九键完整按键 p95 仍为 17.83 ms。`TailRun::resume` 在经过待处理末位置时，先按需计算并缓存末位置单行，再把已有 KV clone/extend 为完整临时前缀；`run_tail_states` 随即复制这个临时前缀到最终存储。首次构建末位置也有完整前缀复制，本片先消除其后每次同步续跑的中间拼接，不把小额数组成本当成主计算瓶颈已解决。

## Decision

内部 `Prefix` 增加可选借用的 `TerminalState`。普通和末位置恢复都借用旧 KV；末位置恢复另外借用已缓存单行，最后隐藏状态仍共享。尾部前向按逻辑长度区分原始可见前缀和可选末行，直接按最终长度构造 K/V。空前向同样保留两段，不漏末行；普通借用只复制可见前缀，不读取未来位置。末位置缓存仍只持有新行，独立上文仍拥有完整 KV，缓存寿命、全部算术、评分与接口保持。

## Existing decisions

已检索 `末位置`、`terminal`、`split_off`、`resume`、`KV` 和借用。[普通前缀借用](../../implemented/testing/2026-10-16-resume-kv-borrow.md)仍持有可见长度边界，本片补上它明确留下的末位置中间复制。[末位置按需前向](../../implemented/bug-fix/2026-10-15-defer-terminal-forward.md)继续要求仅缓存末位置单行；[直接构造最终 KV](../../implemented/testing/2026-10-16-packed-tail-kv.md)仍持有精确容量。没有完全吸收或过时笔记。

## Alternatives considered

- 末位置缓存持有完整前缀：可以直接借用连续数组，改动少；但每个被延长的候选都会长期重复持有原始 KV，违反现有单行缓存内存边界。
- 整体改成分段持久 KV：可以进一步减少首次末位置前向和其它复制；但要改 attention 的索引与状态所有权，本片先减少同步中间拼接。
- 精确预留临时拼接容量：可消除 clone 后的几何扩容，但仍额外复制完整前缀，再被最终存储复制。

## Predeclared criteria

末位置恢复地址回归在旧版真实失败，新版通过。合成多层、多头、Float/Quantized、零层、短长前缀、单字原候选和一至三个后继覆盖完整 KV、隐藏状态、累计分位模式、长度与精确容量；末位置只计算一次，缓存仍只持有每层单行。既有三精度、逐轮增删重排、公开分布与普通截断测试保持。真实模型另做逐位与配对对照；Rust 1.97.1 和最低 1.75、Clippy/fmt、笔记与独立审查作为交付门禁。

完整续跑配对基准冻结 `03b3b5d` 的物化恢复，计时包含 resume、run_tail、joined 与结果释放；固定宽 192，1/6 层，Float/Quantized，已有前缀 1/12 位置、原候选 1/8 字、后继 1/3 字、末状态冷/热共 64 形状。每形状先逐位对照，再用 21 对交替顺序、旧臂校准至少 10 ms；量化六层、热末状态、单后继四个主形状中位 new/old <0.99，其余 <1.05。全部表收集后统一判定，不通过则撤回实现；本次全部通过。正常路径另做真实完整模型与主项目默认质量、九键延迟和 quick；局部通过不代表 16 ms 目标完成。

## Consequences

末位置恢复不再创建两份完整临时前缀及每层 clone/extend，最终存储仍只复制一次。逻辑前缀总长包括可选末行，原数组可见宽度必须少一行。空候选前向、单字候选延长、零层模型和连续多轮拼接易漏边界。私有 Prefix 结构增加一个可选引用，不新增持有型缓存；分支与代码布局可能抵消少量复制收益。所有临时引用只在同步调用内存在，不延长旧轮缓存寿命。

## Measurements

固定基线 `03b3b5db8c6758f5489edd29bf7f5c40ae3bc0b7`，ARM64 release；模型 SHA-256 为 `86ac529510cb3b4968a5e6ade83ec8080f5b34a0a681e75c362accbbd387d1c1`。先把生产 `resume` 暂时还原为固定旧实现，仅适配新字段和私有参数，64 形状完整运行，3 个主形状未过原 0.99 门槛。恢复借用实现后 64 形状全部通过，真实模型 16 形状也全部通过。各轮构建进程每秒观察，红对照 43 次、新版 38 次、真实模型 9 次，构建观察数均为 0；不能排除采样间短进程或其它系统活动。

真实模型已缓存末状态、单后继的四个形状，中位 new/old 为 0.209–0.361，旧中位 2.159–19.302 µs，新中位 0.780–4.010 µs。这是完整恢复与后继计算的局部成本；首次末位置推理仍复制原 KV，矩阵和词表计算保持，不能按此比例折算主项目逐键延迟。

临时公开接口对照从固定提交归档全部 `reference/src`，只把 `crate::` 命名空间适配到测试内的旧模块。使用全拼 60 句和九键 81 句的金标准构造九条共享前缀候选，分别在空与合成上文下进行缩短、延长、重复和倒序；不是引擎录制候选池。141 例、1692 轮、15228 个缓存分数全部逐位一致，冷分数、强制全部候选可比时的首选索引和完整公开分布也一致。临时对照程序已撤出生产测试树，固定旧源和实验程序保存在本地实验记录，永久回归仍使用物化恢复 oracle。

### 旧生产恢复控制完整表

```text
layers=1 quantized=false prefix=1 original=1 continued=1 cold=false repeats=32768 old_us=0.470 new_us=0.470 ratio=0.998535 p10=0.970305 p90=1.003516 budget=1.05
layers=1 quantized=false prefix=1 original=1 continued=1 cold=true repeats=256 old_us=42.028 new_us=42.118 ratio=0.999849 p10=0.977627 p90=1.014913 budget=1.05
layers=1 quantized=false prefix=1 original=1 continued=3 cold=false repeats=128 old_us=83.953 new_us=84.888 ratio=1.004947 p10=0.974113 p90=1.046684 budget=1.05
layers=1 quantized=false prefix=1 original=1 continued=3 cold=true repeats=128 old_us=123.920 new_us=124.101 ratio=1.008520 p10=0.975286 p90=1.032898 budget=1.05
layers=1 quantized=false prefix=1 original=8 continued=1 cold=false repeats=16384 old_us=0.955 new_us=0.951 ratio=0.996734 p10=0.970361 p90=1.020582 budget=1.05
layers=1 quantized=false prefix=1 original=8 continued=1 cold=true repeats=256 old_us=43.860 new_us=43.798 ratio=1.001326 p10=0.963535 p90=1.042989 budget=1.05
layers=1 quantized=false prefix=1 original=8 continued=3 cold=false repeats=128 old_us=84.878 new_us=84.118 ratio=0.990215 p10=0.958150 p90=1.020144 budget=1.05
layers=1 quantized=false prefix=1 original=8 continued=3 cold=true repeats=128 old_us=129.654 new_us=129.493 ratio=1.002456 p10=0.969543 p90=1.033837 budget=1.05
layers=1 quantized=false prefix=12 original=1 continued=1 cold=false repeats=16384 old_us=1.239 new_us=1.220 ratio=0.993619 p10=0.966186 p90=1.019117 budget=1.05
layers=1 quantized=false prefix=12 original=1 continued=1 cold=true repeats=256 old_us=45.300 new_us=45.521 ratio=1.006339 p10=0.958774 p90=1.031561 budget=1.05
layers=1 quantized=false prefix=12 original=1 continued=3 cold=false repeats=128 old_us=88.229 new_us=87.365 ratio=0.995235 p10=0.971987 p90=1.033620 budget=1.05
layers=1 quantized=false prefix=12 original=1 continued=3 cold=true repeats=128 old_us=130.546 new_us=131.057 ratio=1.000405 p10=0.979325 p90=1.028687 budget=1.05
layers=1 quantized=false prefix=12 original=8 continued=1 cold=false repeats=8192 old_us=1.878 new_us=1.870 ratio=0.999339 p10=0.961444 p90=1.055915 budget=1.05
layers=1 quantized=false prefix=12 original=8 continued=1 cold=true repeats=256 old_us=46.691 new_us=46.335 ratio=0.999698 p10=0.973683 p90=1.027386 budget=1.05
layers=1 quantized=false prefix=12 original=8 continued=3 cold=false repeats=128 old_us=87.830 new_us=87.868 ratio=0.996039 p10=0.973692 p90=1.025401 budget=1.05
layers=1 quantized=false prefix=12 original=8 continued=3 cold=true repeats=128 old_us=129.734 new_us=130.515 ratio=1.003555 p10=0.985711 p90=1.015302 budget=1.05
layers=1 quantized=true prefix=1 original=1 continued=1 cold=false repeats=32768 old_us=0.471 new_us=0.475 ratio=1.003191 p10=0.979417 p90=1.026591 budget=1.05
layers=1 quantized=true prefix=1 original=1 continued=1 cold=true repeats=256 old_us=49.551 new_us=49.864 ratio=1.004957 p10=0.973504 p90=1.032942 budget=1.05
layers=1 quantized=true prefix=1 original=1 continued=3 cold=false repeats=128 old_us=97.355 new_us=98.941 ratio=1.007014 p10=0.984140 p90=1.032190 budget=1.05
layers=1 quantized=true prefix=1 original=1 continued=3 cold=true repeats=128 old_us=146.531 new_us=147.234 ratio=0.997541 p10=0.982503 p90=1.019007 budget=1.05
layers=1 quantized=true prefix=1 original=8 continued=1 cold=false repeats=16384 old_us=0.972 new_us=0.970 ratio=0.997727 p10=0.960468 p90=1.035233 budget=1.05
layers=1 quantized=true prefix=1 original=8 continued=1 cold=true repeats=256 old_us=51.145 new_us=51.105 ratio=1.005658 p10=0.978752 p90=1.025856 budget=1.05
layers=1 quantized=true prefix=1 original=8 continued=3 cold=false repeats=128 old_us=100.306 new_us=100.308 ratio=0.996198 p10=0.965463 p90=1.040851 budget=1.05
layers=1 quantized=true prefix=1 original=8 continued=3 cold=true repeats=128 old_us=147.579 new_us=148.010 ratio=1.001308 p10=0.981875 p90=1.029315 budget=1.05
layers=1 quantized=true prefix=12 original=1 continued=1 cold=false repeats=8192 old_us=1.210 new_us=1.219 ratio=1.003502 p10=0.994352 p90=1.022641 budget=1.05
layers=1 quantized=true prefix=12 original=1 continued=1 cold=true repeats=256 old_us=51.553 new_us=51.532 ratio=0.999911 p10=0.991168 p90=1.003235 budget=1.05
layers=1 quantized=true prefix=12 original=1 continued=3 cold=false repeats=128 old_us=101.605 new_us=101.425 ratio=0.997838 p10=0.978767 p90=1.036304 budget=1.05
layers=1 quantized=true prefix=12 original=1 continued=3 cold=true repeats=128 old_us=150.688 new_us=152.322 ratio=1.009638 p10=0.971996 p90=1.035825 budget=1.05
layers=1 quantized=true prefix=12 original=8 continued=1 cold=false repeats=8192 old_us=1.852 new_us=1.882 ratio=1.002901 p10=0.968524 p90=1.066826 budget=1.05
layers=1 quantized=true prefix=12 original=8 continued=1 cold=true repeats=256 old_us=53.104 new_us=52.265 ratio=0.988086 p10=0.961509 p90=1.001210 budget=1.05
layers=1 quantized=true prefix=12 original=8 continued=3 cold=false repeats=128 old_us=102.567 new_us=102.846 ratio=1.007416 p10=0.984020 p90=1.040385 budget=1.05
layers=1 quantized=true prefix=12 original=8 continued=3 cold=true repeats=128 old_us=151.418 new_us=150.914 ratio=0.997577 p10=0.976100 p90=1.011913 budget=1.05
layers=6 quantized=false prefix=1 original=1 continued=1 cold=false repeats=8192 old_us=2.208 new_us=2.209 ratio=0.997883 p10=0.990402 p90=1.013081 budget=1.05
layers=6 quantized=false prefix=1 original=1 continued=1 cold=true repeats=64 old_us=260.967 new_us=259.415 ratio=1.000970 p10=0.971043 p90=1.014122 budget=1.05
layers=6 quantized=false prefix=1 original=1 continued=3 cold=false repeats=32 old_us=501.388 new_us=503.650 ratio=1.006217 p10=0.972992 p90=1.023325 budget=1.05
layers=6 quantized=false prefix=1 original=1 continued=3 cold=true repeats=16 old_us=764.716 new_us=757.471 ratio=0.997100 p10=0.978467 p90=1.029185 budget=1.05
layers=6 quantized=false prefix=1 original=8 continued=1 cold=false repeats=2048 old_us=7.441 new_us=7.457 ratio=0.999837 p10=0.988791 p90=1.016843 budget=1.05
layers=6 quantized=false prefix=1 original=8 continued=1 cold=true repeats=64 old_us=271.659 new_us=271.632 ratio=1.002426 p10=0.990417 p90=1.020192 budget=1.05
layers=6 quantized=false prefix=1 original=8 continued=3 cold=false repeats=32 old_us=517.833 new_us=515.535 ratio=0.991995 p10=0.975415 p90=1.036367 budget=1.05
layers=6 quantized=false prefix=1 original=8 continued=3 cold=true repeats=16 old_us=783.849 new_us=787.643 ratio=0.998445 p10=0.972119 p90=1.038111 budget=1.05
layers=6 quantized=false prefix=12 original=1 continued=1 cold=false repeats=1024 old_us=13.188 new_us=13.412 ratio=1.014085 p10=0.926412 p90=1.116396 budget=1.05
layers=6 quantized=false prefix=12 original=1 continued=1 cold=true repeats=64 old_us=279.509 new_us=279.379 ratio=0.999443 p10=0.962983 p90=1.008332 budget=1.05
layers=6 quantized=false prefix=12 original=1 continued=3 cold=false repeats=32 old_us=514.598 new_us=514.842 ratio=1.001348 p10=0.989059 p90=1.008145 budget=1.05
layers=6 quantized=false prefix=12 original=1 continued=3 cold=true repeats=16 old_us=783.414 new_us=788.815 ratio=0.998453 p10=0.984909 p90=1.019069 budget=1.05
layers=6 quantized=false prefix=12 original=8 continued=1 cold=false repeats=1024 old_us=18.408 new_us=18.834 ratio=1.016602 p10=0.952338 p90=1.097561 budget=1.05
layers=6 quantized=false prefix=12 original=8 continued=1 cold=true repeats=64 old_us=286.128 new_us=286.436 ratio=0.997940 p10=0.989193 p90=1.008162 budget=1.05
layers=6 quantized=false prefix=12 original=8 continued=3 cold=false repeats=32 old_us=526.928 new_us=527.634 ratio=0.996909 p10=0.983233 p90=1.018688 budget=1.05
layers=6 quantized=false prefix=12 original=8 continued=3 cold=true repeats=16 old_us=803.911 new_us=797.174 ratio=0.999289 p10=0.987422 p90=1.024019 budget=1.05
layers=6 quantized=true prefix=1 original=1 continued=1 cold=false repeats=8192 old_us=2.142 new_us=2.140 ratio=0.996150 p10=0.973415 p90=1.019807 budget=0.99
layers=6 quantized=true prefix=1 original=1 continued=1 cold=true repeats=64 old_us=287.363 new_us=287.555 ratio=1.000668 p10=0.993920 p90=1.012553 budget=1.05
layers=6 quantized=true prefix=1 original=1 continued=3 cold=false repeats=32 old_us=577.033 new_us=573.219 ratio=0.997513 p10=0.969928 p90=1.020881 budget=1.05
layers=6 quantized=true prefix=1 original=1 continued=3 cold=true repeats=16 old_us=869.068 new_us=855.620 ratio=0.994005 p10=0.943602 p90=1.040031 budget=1.05
layers=6 quantized=true prefix=1 original=8 continued=1 cold=false repeats=2048 old_us=7.309 new_us=7.395 ratio=1.002572 p10=0.966917 p90=1.057105 budget=0.99
layers=6 quantized=true prefix=1 original=8 continued=1 cold=true repeats=64 old_us=302.184 new_us=300.455 ratio=1.001767 p10=0.972802 p90=1.016401 budget=1.05
layers=6 quantized=true prefix=1 original=8 continued=3 cold=false repeats=32 old_us=586.639 new_us=583.775 ratio=0.997474 p10=0.980379 p90=1.013860 budget=1.05
layers=6 quantized=true prefix=1 original=8 continued=3 cold=true repeats=16 old_us=879.516 new_us=879.146 ratio=0.996727 p10=0.991978 p90=1.010019 budget=1.05
layers=6 quantized=true prefix=12 original=1 continued=1 cold=false repeats=1024 old_us=13.109 new_us=13.039 ratio=0.974232 p10=0.922717 p90=1.035215 budget=0.99
layers=6 quantized=true prefix=12 original=1 continued=1 cold=true repeats=64 old_us=307.004 new_us=308.038 ratio=1.005300 p10=0.981480 p90=1.021159 budget=1.05
layers=6 quantized=true prefix=12 original=1 continued=3 cold=false repeats=32 old_us=596.392 new_us=599.591 ratio=1.003961 p10=0.988105 p90=1.022045 budget=1.05
layers=6 quantized=true prefix=12 original=1 continued=3 cold=true repeats=16 old_us=900.854 new_us=895.214 ratio=1.004092 p10=0.972242 p90=1.031233 budget=1.05
layers=6 quantized=true prefix=12 original=8 continued=1 cold=false repeats=1024 old_us=18.451 new_us=18.749 ratio=1.017024 p10=0.957456 p90=1.079624 budget=0.99
layers=6 quantized=true prefix=12 original=8 continued=1 cold=true repeats=64 old_us=311.808 new_us=312.360 ratio=1.000634 p10=0.985744 p90=1.026711 budget=1.05
layers=6 quantized=true prefix=12 original=8 continued=3 cold=false repeats=32 old_us=604.755 new_us=602.708 ratio=0.996155 p10=0.927433 p90=1.031643 budget=1.05
layers=6 quantized=true prefix=12 original=8 continued=3 cold=true repeats=16 old_us=917.417 new_us=921.435 ratio=1.001704 p10=0.958328 p90=1.032451 budget=1.05
```

### 两段借用完整表

```text
layers=1 quantized=false prefix=1 original=1 continued=1 cold=false repeats=32768 old_us=0.474 new_us=0.219 ratio=0.461165 p10=0.452611 p90=0.465358 budget=1.05
layers=1 quantized=false prefix=1 original=1 continued=1 cold=true repeats=256 old_us=42.775 new_us=42.654 ratio=1.001356 p10=0.972098 p90=1.030590 budget=1.05
layers=1 quantized=false prefix=1 original=1 continued=3 cold=false repeats=128 old_us=85.030 new_us=84.384 ratio=0.994730 p10=0.969171 p90=1.012998 budget=1.05
layers=1 quantized=false prefix=1 original=1 continued=3 cold=true repeats=128 old_us=126.234 new_us=126.472 ratio=1.001741 p10=0.980673 p90=1.017789 budget=1.05
layers=1 quantized=false prefix=1 original=8 continued=1 cold=false repeats=16384 old_us=0.970 new_us=0.379 ratio=0.390223 p10=0.370315 p90=0.398446 budget=1.05
layers=1 quantized=false prefix=1 original=8 continued=1 cold=true repeats=256 old_us=44.871 new_us=43.556 ratio=0.977682 p10=0.946279 p90=1.019146 budget=1.05
layers=1 quantized=false prefix=1 original=8 continued=3 cold=false repeats=128 old_us=86.363 new_us=85.743 ratio=0.991615 p10=0.974771 p90=1.041720 budget=1.05
layers=1 quantized=false prefix=1 original=8 continued=3 cold=true repeats=128 old_us=129.377 new_us=126.945 ratio=0.979146 p10=0.961211 p90=1.003418 budget=1.05
layers=1 quantized=false prefix=12 original=1 continued=1 cold=false repeats=8192 old_us=1.213 new_us=0.449 ratio=0.372360 p10=0.360442 p90=0.380349 budget=1.05
layers=1 quantized=false prefix=12 original=1 continued=1 cold=true repeats=256 old_us=44.951 new_us=43.611 ratio=0.977091 p10=0.952979 p90=0.990981 budget=1.05
layers=1 quantized=false prefix=12 original=1 continued=3 cold=false repeats=128 old_us=86.909 new_us=85.689 ratio=0.983717 p10=0.964891 p90=1.025902 budget=1.05
layers=1 quantized=false prefix=12 original=1 continued=3 cold=true repeats=128 old_us=129.958 new_us=128.854 ratio=0.989875 p10=0.950288 p90=1.013277 budget=1.05
layers=1 quantized=false prefix=12 original=8 continued=1 cold=false repeats=8192 old_us=1.902 new_us=0.633 ratio=0.330038 p10=0.314441 p90=0.335394 budget=1.05
layers=1 quantized=false prefix=12 original=8 continued=1 cold=true repeats=256 old_us=45.949 new_us=44.847 ratio=0.969933 p10=0.943011 p90=0.988456 budget=1.05
layers=1 quantized=false prefix=12 original=8 continued=3 cold=false repeats=128 old_us=88.951 new_us=87.033 ratio=0.976088 p10=0.956567 p90=1.025729 budget=1.05
layers=1 quantized=false prefix=12 original=8 continued=3 cold=true repeats=128 old_us=132.002 new_us=129.915 ratio=0.988502 p10=0.967930 p90=1.012656 budget=1.05
layers=1 quantized=true prefix=1 original=1 continued=1 cold=false repeats=32768 old_us=0.474 new_us=0.225 ratio=0.474649 p10=0.465514 p90=0.481678 budget=1.05
layers=1 quantized=true prefix=1 original=1 continued=1 cold=true repeats=256 old_us=49.867 new_us=48.717 ratio=0.988378 p10=0.954504 p90=1.026201 budget=1.05
layers=1 quantized=true prefix=1 original=1 continued=3 cold=false repeats=128 old_us=97.336 new_us=97.988 ratio=1.004160 p10=0.976548 p90=1.022230 budget=1.05
layers=1 quantized=true prefix=1 original=1 continued=3 cold=true repeats=128 old_us=146.143 new_us=146.613 ratio=1.002540 p10=0.977512 p90=1.024917 budget=1.05
layers=1 quantized=true prefix=1 original=8 continued=1 cold=false repeats=16384 old_us=0.951 new_us=0.385 ratio=0.406613 p10=0.394695 p90=0.416683 budget=1.05
layers=1 quantized=true prefix=1 original=8 continued=1 cold=true repeats=256 old_us=51.304 new_us=49.875 ratio=0.978986 p10=0.938775 p90=1.011593 budget=1.05
layers=1 quantized=true prefix=1 original=8 continued=3 cold=false repeats=128 old_us=100.218 new_us=98.797 ratio=0.985627 p10=0.963993 p90=1.015847 budget=1.05
layers=1 quantized=true prefix=1 original=8 continued=3 cold=true repeats=128 old_us=149.865 new_us=148.788 ratio=0.988439 p10=0.972843 p90=1.012303 budget=1.05
layers=1 quantized=true prefix=12 original=1 continued=1 cold=false repeats=8192 old_us=1.211 new_us=0.453 ratio=0.377298 p10=0.364407 p90=0.385085 budget=1.05
layers=1 quantized=true prefix=12 original=1 continued=1 cold=true repeats=256 old_us=51.591 new_us=50.857 ratio=0.983710 p10=0.961577 p90=0.999502 budget=1.05
layers=1 quantized=true prefix=12 original=1 continued=3 cold=false repeats=128 old_us=100.749 new_us=98.603 ratio=0.986509 p10=0.963405 p90=1.027829 budget=1.05
layers=1 quantized=true prefix=12 original=1 continued=3 cold=true repeats=128 old_us=150.938 new_us=147.879 ratio=0.988364 p10=0.946812 p90=1.008872 budget=1.05
layers=1 quantized=true prefix=12 original=8 continued=1 cold=false repeats=8192 old_us=1.838 new_us=0.602 ratio=0.327619 p10=0.304487 p90=0.331980 budget=1.05
layers=1 quantized=true prefix=12 original=8 continued=1 cold=true repeats=256 old_us=52.948 new_us=51.925 ratio=0.978258 p10=0.956754 p90=0.992158 budget=1.05
layers=1 quantized=true prefix=12 original=8 continued=3 cold=false repeats=128 old_us=102.516 new_us=101.464 ratio=0.988479 p10=0.960786 p90=1.027778 budget=1.05
layers=1 quantized=true prefix=12 original=8 continued=3 cold=true repeats=128 old_us=153.276 new_us=151.009 ratio=0.983763 p10=0.963225 p90=1.013373 budget=1.05
layers=6 quantized=false prefix=1 original=1 continued=1 cold=false repeats=8192 old_us=2.188 new_us=0.818 ratio=0.370911 p10=0.355062 p90=0.380436 budget=1.05
layers=6 quantized=false prefix=1 original=1 continued=1 cold=true repeats=64 old_us=260.616 new_us=259.607 ratio=0.995872 p10=0.982606 p90=1.016898 budget=1.05
layers=6 quantized=false prefix=1 original=1 continued=3 cold=false repeats=32 old_us=506.333 new_us=502.908 ratio=0.992918 p10=0.975484 p90=1.009512 budget=1.05
layers=6 quantized=false prefix=1 original=1 continued=3 cold=true repeats=16 old_us=770.156 new_us=759.961 ratio=0.991735 p10=0.957897 p90=1.014416 budget=1.05
layers=6 quantized=false prefix=1 original=8 continued=1 cold=false repeats=2048 old_us=7.662 new_us=1.902 ratio=0.245376 p10=0.236462 p90=0.267392 budget=1.05
layers=6 quantized=false prefix=1 original=8 continued=1 cold=true repeats=64 old_us=272.042 new_us=267.838 ratio=0.984803 p10=0.971118 p90=1.003562 budget=1.05
layers=6 quantized=false prefix=1 original=8 continued=3 cold=false repeats=32 old_us=516.723 new_us=511.590 ratio=0.987369 p10=0.974739 p90=1.011578 budget=1.05
layers=6 quantized=false prefix=1 original=8 continued=3 cold=true repeats=16 old_us=780.581 new_us=781.130 ratio=0.991036 p10=0.975383 p90=1.025785 budget=1.05
layers=6 quantized=false prefix=12 original=1 continued=1 cold=false repeats=1024 old_us=12.856 new_us=3.050 ratio=0.236605 p10=0.230935 p90=0.246740 budget=1.05
layers=6 quantized=false prefix=12 original=1 continued=1 cold=true repeats=64 old_us=280.743 new_us=270.812 ratio=0.958231 p10=0.948026 p90=0.988033 budget=1.05
layers=6 quantized=false prefix=12 original=1 continued=3 cold=false repeats=32 old_us=520.897 new_us=511.577 ratio=0.979459 p10=0.964715 p90=1.013089 budget=1.05
layers=6 quantized=false prefix=12 original=1 continued=3 cold=true repeats=16 old_us=796.781 new_us=781.057 ratio=0.981499 p10=0.966500 p90=1.005138 budget=1.05
layers=6 quantized=false prefix=12 original=8 continued=1 cold=false repeats=1024 old_us=18.622 new_us=4.075 ratio=0.217966 p10=0.211276 p90=0.225288 budget=1.05
layers=6 quantized=false prefix=12 original=8 continued=1 cold=true repeats=64 old_us=290.855 new_us=279.837 ratio=0.960202 p10=0.946731 p90=0.984643 budget=1.05
layers=6 quantized=false prefix=12 original=8 continued=3 cold=false repeats=32 old_us=528.464 new_us=521.811 ratio=0.986203 p10=0.968188 p90=1.002989 budget=1.05
layers=6 quantized=false prefix=12 original=8 continued=3 cold=true repeats=16 old_us=808.128 new_us=799.742 ratio=0.984614 p10=0.967145 p90=1.017682 budget=1.05
layers=6 quantized=true prefix=1 original=1 continued=1 cold=false repeats=8192 old_us=2.208 new_us=0.815 ratio=0.367085 p10=0.354386 p90=0.376258 budget=0.99
layers=6 quantized=true prefix=1 original=1 continued=1 cold=true repeats=64 old_us=294.937 new_us=289.216 ratio=0.982943 p10=0.953257 p90=1.007207 budget=1.05
layers=6 quantized=true prefix=1 original=1 continued=3 cold=false repeats=32 old_us=572.141 new_us=575.331 ratio=0.999473 p10=0.985419 p90=1.026319 budget=1.05
layers=6 quantized=true prefix=1 original=1 continued=3 cold=true repeats=16 old_us=857.299 new_us=856.346 ratio=0.997824 p10=0.991783 p90=1.002335 budget=1.05
layers=6 quantized=true prefix=1 original=8 continued=1 cold=false repeats=2048 old_us=7.559 new_us=1.892 ratio=0.250164 p10=0.247441 p90=0.256722 budget=0.99
layers=6 quantized=true prefix=1 original=8 continued=1 cold=true repeats=64 old_us=298.653 new_us=293.464 ratio=0.981749 p10=0.970745 p90=0.993376 budget=1.05
layers=6 quantized=true prefix=1 original=8 continued=3 cold=false repeats=32 old_us=582.504 new_us=577.208 ratio=0.991136 p10=0.980665 p90=0.998672 budget=1.05
layers=6 quantized=true prefix=1 original=8 continued=3 cold=true repeats=16 old_us=896.055 new_us=891.479 ratio=0.993719 p10=0.961069 p90=1.030323 budget=1.05
layers=6 quantized=true prefix=12 original=1 continued=1 cold=false repeats=1024 old_us=12.736 new_us=3.004 ratio=0.239242 p10=0.220826 p90=0.247308 budget=0.99
layers=6 quantized=true prefix=12 original=1 continued=1 cold=true repeats=32 old_us=311.710 new_us=302.928 ratio=0.974564 p10=0.942074 p90=1.011095 budget=1.05
layers=6 quantized=true prefix=12 original=1 continued=3 cold=false repeats=32 old_us=602.241 new_us=592.158 ratio=0.980591 p10=0.946218 p90=1.008914 budget=1.05
layers=6 quantized=true prefix=12 original=1 continued=3 cold=true repeats=16 old_us=887.956 new_us=875.534 ratio=0.986503 p10=0.965390 p90=0.998188 budget=1.05
layers=6 quantized=true prefix=12 original=8 continued=1 cold=false repeats=1024 old_us=18.576 new_us=3.942 ratio=0.212169 p10=0.208395 p90=0.214506 budget=0.99
layers=6 quantized=true prefix=12 original=8 continued=1 cold=true repeats=32 old_us=312.419 new_us=300.684 ratio=0.966690 p10=0.931756 p90=0.976754 budget=1.05
layers=6 quantized=true prefix=12 original=8 continued=3 cold=false repeats=32 old_us=598.363 new_us=587.092 ratio=0.983303 p10=0.976757 p90=1.000013 budget=1.05
layers=6 quantized=true prefix=12 original=8 continued=3 cold=true repeats=16 old_us=893.839 new_us=883.992 ratio=0.990846 p10=0.979067 p90=1.002676 budget=1.05
```

### 真实模型完整表

```text
layers=6 quantized=true prefix=1 original=1 continued=1 cold=false repeats=8192 old_us=2.159 new_us=0.780 ratio=0.361362 p10=0.358657 p90=0.363697 budget=0.99
layers=6 quantized=true prefix=1 original=1 continued=1 cold=true repeats=32 old_us=459.770 new_us=457.609 ratio=1.000003 p10=0.974952 p90=1.031592 budget=1.05
layers=6 quantized=true prefix=1 original=1 continued=3 cold=false repeats=16 old_us=911.820 new_us=911.615 ratio=0.999737 p10=0.995657 p90=1.020659 budget=1.05
layers=6 quantized=true prefix=1 original=1 continued=3 cold=true repeats=8 old_us=1370.005 new_us=1370.599 ratio=0.998763 p10=0.991570 p90=1.026558 budget=1.05
layers=6 quantized=true prefix=1 original=8 continued=1 cold=false repeats=2048 old_us=7.596 new_us=1.766 ratio=0.233238 p10=0.230031 p90=0.237827 budget=0.99
layers=6 quantized=true prefix=1 original=8 continued=1 cold=true repeats=32 old_us=469.674 new_us=463.988 ratio=0.987568 p10=0.978203 p90=0.995941 budget=1.05
layers=6 quantized=true prefix=1 original=8 continued=3 cold=false repeats=16 old_us=923.857 new_us=916.820 ratio=0.992783 p10=0.978941 p90=0.999124 budget=1.05
layers=6 quantized=true prefix=1 original=8 continued=3 cold=true repeats=8 old_us=1383.469 new_us=1379.062 ratio=0.996944 p10=0.985155 p90=1.018222 budget=1.05
layers=6 quantized=true prefix=12 original=1 continued=1 cold=false repeats=1024 old_us=12.072 new_us=2.659 ratio=0.220737 p10=0.218536 p90=0.224400 budget=0.99
layers=6 quantized=true prefix=12 original=1 continued=1 cold=true repeats=32 old_us=475.759 new_us=467.194 ratio=0.981418 p10=0.979507 p90=0.992206 budget=1.05
layers=6 quantized=true prefix=12 original=1 continued=3 cold=false repeats=16 old_us=928.013 new_us=919.990 ratio=0.991919 p10=0.988225 p90=0.994440 budget=1.05
layers=6 quantized=true prefix=12 original=1 continued=3 cold=true repeats=8 old_us=1390.510 new_us=1384.307 ratio=0.995264 p10=0.993137 p90=0.998730 budget=1.05
layers=6 quantized=true prefix=12 original=8 continued=1 cold=false repeats=1024 old_us=19.302 new_us=4.010 ratio=0.208956 p10=0.205885 p90=0.213812 budget=0.99
layers=6 quantized=true prefix=12 original=8 continued=1 cold=true repeats=32 old_us=481.145 new_us=469.733 ratio=0.975529 p10=0.969038 p90=0.980687 budget=1.05
layers=6 quantized=true prefix=12 original=8 continued=3 cold=false repeats=16 old_us=936.823 new_us=926.146 ratio=0.991063 p10=0.979566 p90=0.998439 budget=1.05
layers=6 quantized=true prefix=12 original=8 continued=3 cold=true repeats=8 old_us=1403.641 new_us=1391.854 ratio=0.992124 p10=0.974023 p90=0.998404 budget=1.05
```

## Verification

Rust 1.97.1 和最低 Rust 1.75.0 各通过 40 项单测、15 项模型文件测试与 doc tests，7 个实际模型或性能手动项默认忽略。324 组合成几何包含宽 4/12/64、1/2 头、0/1/3 层、Float/Quantized、上文 1/4/12 位置和原候选 1/3/8 字；每组比较一至三个后继及再拼接两轮的全部 KV、隐藏状态和累计分位模式。最终 KV 长度与容量相等，末位置缓存仍仅持有每层一行；重复恢复没有额外前向。普通位置截断 oracle 保留独立物化路径，零层模型也覆盖。

`cargo clippy --manifest-path reference/Cargo.toml --all-targets -- -D warnings`、Rustfmt 与 `git diff --check` 通过。[Cow 官方文档](https://doc.rust-lang.org/std/borrow/enum.Cow.html)确认不可变借用不复制；本片不使用新增版本接口，最低工具链实编通过。首次末位置按需计算仍保留完整 KV 构造后拆出单行的原路径，本片不宣称消除该首次复制。


### 整模型串行基准完整记录

固定同一真实模型、九键 81 句资源和公开 `bench` example；先编译固定旧提交的真实完整源码并保存 executable，再恢复新版、重编和复核源码。下表顺序为旧版首轮、新版、旧版重测，均退出 0。它们没有交替配对，旧版两轮分别在 52/67 次每秒进程采样中观察到 184/19 个构建进程（计数可在一次采样内累加多个进程），新版 52 次采样未观察到构建；不能排除其它系统活动。旧版首轮和重测都有外部构建干扰，新版也有尾部噪声，不能把这些串行差值解释为稳定整体收益。15 字列因资源没有符合长度的候选而为零，不是零成本计算。排名打印三轮一致。

#### 旧版首轮

进程采样：`{"samples": 52, "concurrent_build_observations": 184, "max_builds": 7}`。

```text
load 3.755958ms for 4.5 MB
attribution: Trained on the Chinese portion of C4 (ODC-BY, https://huggingface.co/datasets/allenai/c4) and LCCC (MIT, https://github.com/thu-coai/CDial-GPT).
81 sentences from resources/eval/sentences-nine-key-v1.tsv

one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.71 /  0.87   0.93 /  1.16   1.81 /  1.84   2.71 /  2.76   3.61 /  3.64   4.53 /  4.61   5.43 /  5.48   0.00 /  0.00
         3 cand   0.45 /  0.47   1.87 /  1.94   4.54 /  4.69   7.25 /  7.48   9.98 / 10.29  12.97 / 13.16  15.91 / 16.70   0.00 /  0.00
         5 cand   0.47 /  0.48   2.81 /  2.84   7.46 /  7.92  12.13 / 12.55  16.82 / 17.45  21.42 / 21.74  26.27 / 27.37   0.00 /  0.00
         9 cand   0.49 /  0.66   4.72 /  4.80  13.11 / 14.97  21.60 / 24.39  30.85 / 36.31  38.60 / 49.49  47.09 / 52.09   0.00 /  0.00

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         5 cand  0.000 / 0.001  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.000 / 0.000

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.001 / 0.002  0.001 / 0.002  0.002 / 0.003  0.003 / 0.005  0.007 / 0.010  0.005 / 0.008  0.000 / 0.000
         3 cand  0.001 / 0.001  0.003 / 0.004  0.006 / 0.006  0.008 / 0.015  0.010 / 0.014  0.012 / 0.022  0.012 / 0.016  0.000 / 0.000
         5 cand  0.001 / 0.001  0.005 / 0.005  0.009 / 0.010  0.013 / 0.024  0.017 / 0.019  0.021 / 0.025  0.020 / 0.027  0.000 / 0.000
         9 cand  0.001 / 0.003  0.009 / 0.009  0.017 / 0.018  0.025 / 0.035  0.029 / 0.040  0.043 / 0.049  0.038 / 0.046  0.000 / 0.000

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.47            0.47            0.47
         2          5.18            4.70            2.59
         4         27.15           13.03            6.79
         6         65.99           21.50           11.00
         8        121.85           30.29           15.23
        10        193.60           38.18           19.36
        12        278.81           45.46           23.23
        15        276.48           45.15           18.43

slowest keystroke 45.46ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.

ranking check:
   -2.6724  我今天去上班
   -4.1121  我今天去上版
   -4.9305  握今天去上班
   -5.0580  我今天趣上班
   -7.6756  握紧天趣上班
```

#### 新版

进程采样：`{"samples": 52, "concurrent_build_observations": 0, "max_builds": 0}`。

```text
load 3.241041ms for 4.5 MB
attribution: Trained on the Chinese portion of C4 (ODC-BY, https://huggingface.co/datasets/allenai/c4) and LCCC (MIT, https://github.com/thu-coai/CDial-GPT).
81 sentences from resources/eval/sentences-nine-key-v1.tsv

one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.47 /  0.48   0.92 /  0.94   1.82 /  1.88   2.71 /  2.76   3.61 /  3.65   4.53 /  4.59   5.44 /  5.59   0.00 /  0.00
         3 cand   0.47 /  0.48   1.87 /  1.92   4.54 /  4.67   7.23 /  7.31   9.94 / 10.06  12.68 / 12.81  15.39 / 15.55   0.00 /  0.00
         5 cand   0.46 /  0.48   3.13 /  7.48   8.73 / 12.36  12.79 / 14.36  16.30 / 16.56  21.41 / 22.15  25.99 / 36.02   0.00 /  0.00
         9 cand   0.47 /  0.76   4.70 /  5.56  12.99 / 13.46  20.93 / 21.48  29.46 / 38.79  37.40 / 41.28  45.41 / 46.76   0.00 /  0.00

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         5 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.002  0.002 / 0.002  0.002 / 0.002  0.000 / 0.000

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.002  0.002 / 0.004  0.003 / 0.007  0.004 / 0.008  0.004 / 0.006  0.000 / 0.000
         3 cand  0.001 / 0.001  0.003 / 0.004  0.005 / 0.006  0.008 / 0.017  0.009 / 0.016  0.012 / 0.015  0.012 / 0.014  0.000 / 0.000
         5 cand  0.001 / 0.001  0.005 / 0.007  0.009 / 0.010  0.012 / 0.016  0.015 / 0.017  0.020 / 0.025  0.019 / 0.022  0.000 / 0.000
         9 cand  0.001 / 0.001  0.009 / 0.009  0.016 / 0.017  0.023 / 0.030  0.027 / 0.030  0.034 / 0.040  0.035 / 0.040  0.000 / 0.000

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.46            0.46            0.46
         2          5.02            4.56            2.51
         4         26.69           12.89            6.67
         6         64.68           21.22           10.78
         8        118.54           28.92           14.82
        10        187.64           37.06           18.76
        12        274.39           45.21           22.87
        15        274.35           45.31           18.29

slowest keystroke 45.31ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.

ranking check:
   -2.6724  我今天去上班
   -4.1121  我今天去上版
   -4.9305  握今天去上班
   -5.0580  我今天趣上班
   -7.6756  握紧天趣上班
```

#### 旧版重测

进程采样：`{"samples": 67, "concurrent_build_observations": 19, "max_builds": 2}`。

```text
load 4.250375ms for 4.5 MB
attribution: Trained on the Chinese portion of C4 (ODC-BY, https://huggingface.co/datasets/allenai/c4) and LCCC (MIT, https://github.com/thu-coai/CDial-GPT).
81 sentences from resources/eval/sentences-nine-key-v1.tsv

one decision, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand   0.48 /  2.10   0.93 /  3.83   1.86 /  1.87   2.79 /  2.79   3.72 / 11.14   4.68 / 14.94   5.61 / 24.30   0.00 /  0.00
         3 cand   0.47 /  0.47   1.87 /  3.25   4.68 / 11.16   7.43 / 13.63  10.21 / 22.21  13.00 / 18.18  15.81 / 29.10   0.00 /  0.00
         5 cand   0.47 /  0.47   2.80 /  2.81   7.45 / 17.47  12.07 / 34.04  16.73 / 33.89  21.36 / 44.80  26.06 / 90.12   0.00 /  0.00
         9 cand   0.47 /  1.02   4.71 /  8.80  13.04 / 51.56  21.40 / 45.41  29.75 / 96.17  38.88 /100.80  46.98 /116.03   0.00 /  0.00

unchanged decision with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000
         3 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         5 cand  0.000 / 0.000  0.000 / 0.000  0.000 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.000 / 0.000
         9 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.001 / 0.001  0.002 / 0.002  0.002 / 0.002  0.002 / 0.002  0.000 / 0.000

changing final character with a warm Reranker, milliseconds (p50 / p95 over 40 runs):
      chars:             1             2             4             6             8            10            12            15
         1 cand  0.001 / 0.001  0.001 / 0.001  0.001 / 0.002  0.002 / 0.003  0.003 / 0.004  0.005 / 0.008  0.005 / 0.008  0.000 / 0.000
         3 cand  0.001 / 0.001  0.003 / 0.004  0.006 / 0.006  0.008 / 0.016  0.010 / 0.014  0.012 / 0.022  0.014 / 0.018  0.000 / 0.000
         5 cand  0.001 / 0.001  0.005 / 0.005  0.009 / 0.010  0.013 / 0.022  0.017 / 0.020  0.022 / 0.028  0.020 / 0.026  0.000 / 0.000
         9 cand  0.001 / 0.001  0.009 / 0.010  0.018 / 0.018  0.027 / 0.037  0.030 / 0.040  0.040 / 0.045  0.039 / 0.046  0.000 / 0.000

typing one sentence with 9 candidates, rescored every keystroke:
     chars      total ms    worst key ms     mean key ms
         1          0.47            0.47            0.47
         2          5.18            4.69            2.59
         4         27.35           13.04            6.84
         6         66.01           21.43           11.00
         8        121.39           29.79           15.17
        10        193.57           38.22           19.36
        12        283.34           46.94           23.61
        15        282.93           46.82           18.86

slowest keystroke 46.94ms. Pass --budget-ms to fail when it is too slow;
what counts as too slow belongs to the host, not to this crate.

ranking check:
   -2.6724  我今天去上班
   -4.1121  我今天去上版
   -4.9305  握今天去上班
   -5.0580  我今天趣上班
   -7.6756  握紧天趣上班
```


## Review

独立审查覆盖所有权与生命周期、末行可见宽度、空前向和零层、普通截断、固定旧版 oracle、计时边界、冷状态对称与完整配对门禁，无阻断或重要问题。末位置缓存不持有前缀，借用不逃逸到下一轮。实际模型手动测试未将摘要写死为代码断言，本轮另行重算 SHA-256 与配置（vocab 8192、6 层、4 头、宽 192、context 64）核实固定资源；后续换模型需要重新注明资源和几何。
