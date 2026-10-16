# Agent Note: 两个输入位置共享量化权重转换

Status: rejected — 最终安全 Rust 原型完整线性层预算失败，恢复生产实现

## Problem

主项目固定 `9920c262fa3ed72c931ee4c12ce6c94526844055`，模型固定 `6acc7839572fb7df221a8e4910467c9dedd72fb8`。上一片无构建观察的九键 ABBA 模型 p95 为 17.36/17.49/17.29/17.61 ms，16 ms 仍未达到。此前实际调用采样中 linear_into 与 dot_i8 是主要模型热点；采样期间有外部构建，只定位热点，不作延迟预算依据。正式 ARM64 release 反汇编确认精度已在循环外分派，内环使用八部分和及 tbl/scvtf/fmul/fadd 向量指令，不再假设缺少向量化。

临时计数在完整九键 81 例、1791 键、5 预热例中聚合每次 linear_into 的输入宽、输出宽、输入位置数及精度；不记录输入文本或权重。共有 136776 次调用、44 种形状，全部量化。单位置 78312 次、多位置 58464 次；乘法项分别为 8660680704 与 21870673920，多位置占 71.6335%。这个比例是几何工作量，不是时间占比。每种投影几何的行数 1..11 调用数依次为 19578/4242/5574/1974/1572/684/348/114/66/30/12，四种几何为 192→576、192→192、192→768、768→192。计数器与临时 Cargo path patch 已逐字撤回，计数运行的延迟不作成绩。

## Proposal

仅对量化线性层的相邻两个输入位置一起计算同一输出行：一份八元素权重分块转 f32 后，分别更新两套原八部分和。每个输出保留原 lane 对应、加法顺序、尾部、归约、scale 与 bias 运算；不使用 mul_add。剩余奇数位置沿用旧点积，浮点和单位置保留原实现。只增加两个固定八元素累加数组，不申请临时权重行，不展开常驻矩阵，不新增依赖、unsafe 或架构分支，保持 Rust 1.75。

## Existing decisions

已检索 linear_into、dot_i8、量化、交错、输入行、转换及缓存。与[输出行分组负结果](../../rejected/bug-fix/2026-10-15-matrix-row-batching.md)部分重叠：该方案同一输入对四个不同权重行计算，不能共享权重转换，本片用两个输入对同一权重行。与[临时行展开负结果](../../rejected/testing/2026-10-17-linear-temporary-row-dequant.md)部分重叠：其整行临时转换增加申请并跨全部输入位置，本片仅在寄存器分块共享两位置，不采用失败后的几何白名单。与[精度分派负结果](../../rejected/bug-fix/2026-10-15-matrix-row-dispatch.md)、[直接行分派调查](../../rejected/testing/2026-10-16-linear-direct-rows.md)部分相关，不重做已被机器码否定的分派假设。[前馈缓冲](../../implemented/testing/2026-10-16-feed-forward-scratch.md)继续持有缓冲寿命。没有完全吸收的笔记或过时提案。

[slice 文档](https://doc.rust-lang.org/std/primitive.slice.html#method.chunks_exact)核对完整块与 remainder；[gemmlowp GEMV 设计](https://github.com/google/gemmlowp/blob/master/todo/fast-gemv.txt)提供复用加载及独立累加链的动机，不引入其代码。实际编译是否保留转换复用与是否带来收益都另行验证。

## Alternatives considered

- 临时整行展开后遍历所有输入：可最大化转换共享，但此前完整线性层未过门槛，不重复或按失败结果筛形状。
- 继续改普通行遍历或更多累加路数：代码局部，然而分派已外提，既有更多路数和四输出分组均没有可靠收益；本次针对有实际多输入工作量的转换共享。
- 手写架构内核或常驻展开权重：可更直接控制计算，但违背当前 unsafe 与键盘扩展常驻内存约束，保留安全 Rust 及原量化表示。

## Acceptance criteria

先冻结完整旧 linear 与写入 helper；旧生产入口运行同一预算必须真实失败，再写新实现。完整 linear 计时包含输出申请、scale、bias、写入和析构；基宽 192/768 × qkv/proj/fc/out × 输入行 1/2/3/4/5/12 × Float/Quantized 共 96 形状，每形状 21 对交替先后，最快批校准至少 50 ms。量化且行数>=2 的每形状中位 new/old 必须 <0.95，其余每形状 <1.05；收齐后统一断言，完整保留失败、不挑选形状或放宽门槛。

逐位对照宽 1/7/8/9/192/768、空行、零输出、奇偶输入/输出、输入不完整尾部、负值与非有限值；正式模型评分和公开分布逐位保持。通过后运行完整模型 bench 整表、当前 Rust 与 1.75、全套测试、Clippy、fmt、笔记和独立审查；上游单独提交合并后才固定合并提交接入主项目，验证质量、九键 ABBA 与 quick，再单独合并接入 PR。失败则恢复生产，仅提交值得保留的负结果。

## Risks

编译器可能跨输入重排寄存器，增加 shuffle 或破坏转换复用；两个位置的输出写入跨行，成本可能抵消收益。单输入控制要涵盖新增分支；奇数行回退必须保留。局部预算通过也不能证明 16 ms 达标。

## Code generation investigation

本机为 Apple M1 Max、macOS 27.0.1 (26A434)、aarch64-apple-darwin；rustc 1.97.1 (8bab26f4f68e0e26f0bb7960be334d5b520ea452)、LLVM 22.1.6，Cargo 默认 release profile，无额外 target-cpu 或 fast-math 设置。

先完成冻结旧入口的 96 形状预算：329.11 秒、退出 101，40 个量化多位置目标均未达到 <0.95，另有 Float/192/fc 的 rows=3 与 rows=5 控制失败，ratio 分别为 1.07086442、1.11426479。315 个每秒监测样本累计 498 个构建进程观察，最多 11 个；这些控制失败不能归因于旧实现退化，也不把本轮视为空闲稳定基线。监测只计进程名，不记录其它会话命令或路径；没有停止其它会话。

逐位测试最初采用小 dyadic 数，审查指出它可能无法识别有限累加的舍入顺序变化，因此在第一次生产修改前增加宽指数、非整数尾数、正负相消、独立 bias 及非二幂 scale。这个补强没有改变计时夹具、96 形状或门槛。独立的 f32 舍入模拟确认 192 宽夹具能识别八路改串行累加；真实生产原型仍以 Rust 逐位测试结果为准。

三个生产表达分别为：v1 在同一 lane 循环交错更新两个累加数组；v2 先建立当前八元素转换值，再分开两套 lane 更新循环；v3 在 v2 上给点积函数加 inline(never)。v1/v2 的新增 pair 内环仍被编译器组合成跨输入位置的两 lane 运算，逐元素标量转换并伴随 zip 指令；旧单行和回退内环继续保留 tbl/scvtf.4s，不能说整个 linear 失去向量化。v3 独立 pair 内环有八个标量 scvtf，没有 tbl/scvtf.4s，仍出现 zip。三种表达均通过普通和有限舍入逐位检查，v1/v2 只有工具回显，没有单独的测试日志文件；v3 在完整预算后另外保留两项逐位测试通过的日志。v1/v2 的性能调查只有机器码检查，未执行计时门禁，不能给它们量化性能结论；仅 v3 执行完整 96 形状预算。三次代码生成检查后停止继续盲调，重新评估循环组织。

## Measurements and decision

red 完整收齐 96 形状、2016 对，耗时 329.11 秒，预算退出 101；共 42 形状失败，其中目标 40、控制 2。构建监测为 `{"samples": 315, "concurrent_build_observations": 498, "max_builds": 11}`。

v3 完整收齐 96 形状、2016 对，耗时 439.18 秒，预算退出 101；共 40 形状失败，其中目标 40、控制 0。构建监测为 `{"samples": 419, "concurrent_build_observations": 1030, "max_builds": 20}`。

量化多位置没有通过事先冻结的逐形状门槛，因此恢复全部生产实现并删除临时测试注册和测试文件。不筛除失败形状、不按结果增加几何白名单、不放宽预算，也不重复跑同一版本寻找绿灯。三种表达的机器码与最终完整门禁不支持将本方案接入主项目；这不证明所有跨输入位置共享方式都不可能，仅拒绝当前编译器和 ARM64 上这三种安全 Rust 表达。受构建或其它活动影响的样本不能用来宣称稳定回退倍数，未观察到构建也不代表完全没有系统活动。每个表格保留全部失败，不能把几何均值代替门禁。

没有运行改版完整模型 bench、正式模型评分或分布、MSRV、主项目质量及九键 ABBA；因为没有交付前向改动，这些未执行项不会被写成通过。九键 16 ms 目标仍未达成，不能从本次局部实验声称端到端改善或回退。恢复后的源码与固定基线逐字一致，另在恢复后运行普通全测、Clippy、fmt 和笔记校验。

[全部原始每臂时间](../../../../docs/measurements/2026-10-17-linear-paired-inputs.csv)共 4032 对，按 run/精度/基宽/投影/行数/对号完整保存；都是合成数值几何，不含真实输入或权重。每臂秒数包含批量 repeats 次完整 linear 的申请、scale、bias、写入和释放。以下 old_us/new_us 是各臂逐次耗时的中位数；ratio 是 21 个逐对 new_s/old_s 的中位数，p10/p90 分别取排序后第 3/19 个值。门槛严格小于，ratio >= budget 就失败。CSV 保留九位小数秒，与原始打印精度一致。校准每轮只测一对，取当时两臂中较快者达到 50 ms；它不保证后续全部正式批次都达到 50 ms，受调度干扰时尤其如此，这是本实验的限制，不能通过事后改门槛或挑样本消除。

### red 全表

| 精度 | 基宽 | 投影 | 行数 | old_us | new_us | ratio | p10 | p90 | budget | 结果 |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| i8 | 192 | qkv | 1 | 10.907 | 10.936 | 1.00527343 | 0.991447 | 1.010332 | 1.05 | 通过 |
| i8 | 192 | qkv | 2 | 21.762 | 21.789 | 1.00065691 | 0.994936 | 1.010484 | 0.95 | 失败 |
| i8 | 192 | qkv | 3 | 32.680 | 32.609 | 0.99681335 | 0.989896 | 1.005175 | 0.95 | 失败 |
| i8 | 192 | qkv | 4 | 43.346 | 43.306 | 0.99879961 | 0.994080 | 1.006084 | 0.95 | 失败 |
| i8 | 192 | qkv | 5 | 54.124 | 54.150 | 1.00029829 | 0.990529 | 1.012327 | 0.95 | 失败 |
| i8 | 192 | qkv | 12 | 129.727 | 130.063 | 1.00124430 | 0.986332 | 1.008354 | 0.95 | 失败 |
| i8 | 192 | proj | 1 | 3.606 | 3.596 | 0.99851766 | 0.989597 | 1.008127 | 1.05 | 通过 |
| i8 | 192 | proj | 2 | 7.203 | 7.211 | 1.00163238 | 0.987974 | 1.007629 | 0.95 | 失败 |
| i8 | 192 | proj | 3 | 10.858 | 10.817 | 0.99512043 | 0.989011 | 1.010107 | 0.95 | 失败 |
| i8 | 192 | proj | 4 | 14.396 | 14.395 | 1.00207388 | 0.991631 | 1.009906 | 0.95 | 失败 |
| i8 | 192 | proj | 5 | 17.993 | 17.975 | 0.99698450 | 0.986294 | 1.004342 | 0.95 | 失败 |
| i8 | 192 | proj | 12 | 43.135 | 43.167 | 1.00154757 | 0.995753 | 1.009410 | 0.95 | 失败 |
| i8 | 192 | fc | 1 | 14.523 | 14.543 | 1.00027194 | 0.985873 | 1.008763 | 1.05 | 通过 |
| i8 | 192 | fc | 2 | 28.958 | 28.965 | 0.99998595 | 0.994282 | 1.009713 | 0.95 | 失败 |
| i8 | 192 | fc | 3 | 43.351 | 43.372 | 0.99942829 | 0.985805 | 1.009600 | 0.95 | 失败 |
| i8 | 192 | fc | 4 | 57.792 | 57.736 | 0.99912546 | 0.991531 | 1.006526 | 0.95 | 失败 |
| i8 | 192 | fc | 5 | 72.446 | 72.416 | 1.00009327 | 0.994188 | 1.006755 | 0.95 | 失败 |
| i8 | 192 | fc | 12 | 173.467 | 173.660 | 1.00011552 | 0.992585 | 1.008367 | 0.95 | 失败 |
| i8 | 192 | out | 1 | 15.279 | 15.277 | 1.00371161 | 0.995193 | 1.009470 | 1.05 | 通过 |
| i8 | 192 | out | 2 | 30.671 | 30.711 | 0.99939725 | 0.990206 | 1.012944 | 0.95 | 失败 |
| i8 | 192 | out | 3 | 45.980 | 45.985 | 0.99980117 | 0.996381 | 1.023510 | 0.95 | 失败 |
| i8 | 192 | out | 4 | 61.240 | 61.214 | 0.99956511 | 0.994972 | 1.010586 | 0.95 | 失败 |
| i8 | 192 | out | 5 | 76.459 | 76.491 | 0.99905341 | 0.991691 | 1.004910 | 0.95 | 失败 |
| i8 | 192 | out | 12 | 183.209 | 183.656 | 1.00125880 | 0.993609 | 1.008983 | 0.95 | 失败 |
| i8 | 768 | qkv | 1 | 182.822 | 183.060 | 1.00063031 | 0.997056 | 1.007744 | 1.05 | 通过 |
| i8 | 768 | qkv | 2 | 365.729 | 365.040 | 1.00057054 | 0.987859 | 1.012825 | 0.95 | 失败 |
| i8 | 768 | qkv | 3 | 547.519 | 547.208 | 0.99839078 | 0.994250 | 1.002764 | 0.95 | 失败 |
| i8 | 768 | qkv | 4 | 730.943 | 731.595 | 1.00268537 | 0.995932 | 1.006843 | 0.95 | 失败 |
| i8 | 768 | qkv | 5 | 922.251 | 927.852 | 0.99922949 | 0.992921 | 1.018019 | 0.95 | 失败 |
| i8 | 768 | qkv | 12 | 2239.141 | 2236.324 | 0.99957344 | 0.990939 | 1.008389 | 0.95 | 失败 |
| i8 | 768 | proj | 1 | 61.486 | 61.379 | 0.99892386 | 0.979368 | 1.011563 | 1.05 | 通过 |
| i8 | 768 | proj | 2 | 123.167 | 122.301 | 0.99839651 | 0.983945 | 1.006249 | 0.95 | 失败 |
| i8 | 768 | proj | 3 | 182.673 | 183.316 | 1.00275360 | 0.990457 | 1.008736 | 0.95 | 失败 |
| i8 | 768 | proj | 4 | 243.723 | 243.253 | 0.99794007 | 0.987220 | 1.009068 | 0.95 | 失败 |
| i8 | 768 | proj | 5 | 305.983 | 304.243 | 0.99628448 | 0.989630 | 1.000921 | 0.95 | 失败 |
| i8 | 768 | proj | 12 | 733.553 | 731.160 | 0.99814566 | 0.992563 | 1.001422 | 0.95 | 失败 |
| i8 | 768 | fc | 1 | 244.372 | 244.339 | 0.99739376 | 0.977171 | 1.008337 | 1.05 | 通过 |
| i8 | 768 | fc | 2 | 487.915 | 488.049 | 1.00045592 | 0.992371 | 1.012910 | 0.95 | 失败 |
| i8 | 768 | fc | 3 | 733.295 | 733.732 | 0.99996446 | 0.990915 | 1.015925 | 0.95 | 失败 |
| i8 | 768 | fc | 4 | 975.650 | 975.566 | 0.99794669 | 0.995033 | 1.005094 | 0.95 | 失败 |
| i8 | 768 | fc | 5 | 1221.214 | 1220.555 | 1.00035985 | 0.982357 | 1.007062 | 0.95 | 失败 |
| i8 | 768 | fc | 12 | 2929.616 | 2928.484 | 0.99983243 | 0.992596 | 1.007049 | 0.95 | 失败 |
| i8 | 768 | out | 1 | 275.665 | 276.464 | 1.00111153 | 0.995447 | 1.018081 | 1.05 | 通过 |
| i8 | 768 | out | 2 | 562.310 | 562.059 | 1.00138152 | 0.989148 | 1.012712 | 0.95 | 失败 |
| i8 | 768 | out | 3 | 828.669 | 828.569 | 0.99958772 | 0.984246 | 1.013205 | 0.95 | 失败 |
| i8 | 768 | out | 4 | 1110.040 | 1108.133 | 1.00016167 | 0.992253 | 1.005981 | 0.95 | 失败 |
| i8 | 768 | out | 5 | 1402.712 | 1397.386 | 0.99734408 | 0.920073 | 1.023554 | 0.95 | 失败 |
| i8 | 768 | out | 12 | 3315.826 | 3304.862 | 0.99701311 | 0.985205 | 1.007615 | 0.95 | 失败 |
| f32 | 192 | qkv | 1 | 8.760 | 8.797 | 1.00151582 | 0.994760 | 1.013925 | 1.05 | 通过 |
| f32 | 192 | qkv | 2 | 18.134 | 18.126 | 1.00689207 | 0.980263 | 1.054584 | 1.05 | 通过 |
| f32 | 192 | qkv | 3 | 28.183 | 28.287 | 0.99926933 | 0.992008 | 1.037015 | 1.05 | 通过 |
| f32 | 192 | qkv | 4 | 37.382 | 37.780 | 0.99939774 | 0.990207 | 1.088597 | 1.05 | 通过 |
| f32 | 192 | qkv | 5 | 46.282 | 46.288 | 0.99830529 | 0.789905 | 1.039342 | 1.05 | 通过 |
| f32 | 192 | qkv | 12 | 111.018 | 110.765 | 0.99804564 | 0.989003 | 1.007285 | 1.05 | 通过 |
| f32 | 192 | proj | 1 | 3.102 | 3.093 | 0.99735399 | 0.983876 | 1.010971 | 1.05 | 通过 |
| f32 | 192 | proj | 2 | 6.170 | 6.135 | 0.99834232 | 0.985058 | 1.006182 | 1.05 | 通过 |
| f32 | 192 | proj | 3 | 9.277 | 9.292 | 1.00092293 | 0.993596 | 1.010234 | 1.05 | 通过 |
| f32 | 192 | proj | 4 | 12.413 | 12.416 | 0.99914966 | 0.986898 | 1.008369 | 1.05 | 通过 |
| f32 | 192 | proj | 5 | 16.433 | 17.205 | 0.99782947 | 0.680573 | 1.732770 | 1.05 | 通过 |
| f32 | 192 | proj | 12 | 49.565 | 55.978 | 1.00117869 | 0.642404 | 2.244504 | 1.05 | 通过 |
| f32 | 192 | fc | 1 | 16.525 | 12.118 | 1.00007606 | 0.492636 | 1.276986 | 1.05 | 通过 |
| f32 | 192 | fc | 2 | 32.172 | 32.710 | 1.00571178 | 0.370555 | 2.043195 | 1.05 | 通过 |
| f32 | 192 | fc | 3 | 53.279 | 45.355 | 1.07086442 | 0.662589 | 1.694731 | 1.05 | 失败 |
| f32 | 192 | fc | 4 | 58.967 | 49.083 | 0.95606865 | 0.424188 | 1.107641 | 1.05 | 通过 |
| f32 | 192 | fc | 5 | 69.568 | 112.912 | 1.11426479 | 0.839229 | 2.628180 | 1.05 | 失败 |
| f32 | 192 | fc | 12 | 148.519 | 148.862 | 1.00366062 | 0.228674 | 1.011898 | 1.05 | 通过 |
| f32 | 192 | out | 1 | 13.961 | 13.934 | 1.00169768 | 0.603098 | 1.597412 | 1.05 | 通过 |
| f32 | 192 | out | 2 | 27.821 | 27.792 | 1.00187130 | 0.978466 | 1.046259 | 1.05 | 通过 |
| f32 | 192 | out | 3 | 40.866 | 40.749 | 0.99855506 | 0.986295 | 1.007151 | 1.05 | 通过 |
| f32 | 192 | out | 4 | 54.825 | 54.488 | 0.99870494 | 0.683054 | 1.076641 | 1.05 | 通过 |
| f32 | 192 | out | 5 | 80.003 | 86.804 | 0.99661097 | 0.525739 | 2.633314 | 1.05 | 通过 |
| f32 | 192 | out | 12 | 272.534 | 302.465 | 1.00698339 | 0.673145 | 1.994972 | 1.05 | 通过 |
| f32 | 768 | qkv | 1 | 202.394 | 223.228 | 1.03051356 | 0.682520 | 1.491603 | 1.05 | 通过 |
| f32 | 768 | qkv | 2 | 336.206 | 335.242 | 0.99777533 | 0.987380 | 1.023544 | 1.05 | 通过 |
| f32 | 768 | qkv | 3 | 499.373 | 499.502 | 0.99917071 | 0.987354 | 1.008699 | 1.05 | 通过 |
| f32 | 768 | qkv | 4 | 667.954 | 669.424 | 1.00101211 | 0.983133 | 1.023321 | 1.05 | 通过 |
| f32 | 768 | qkv | 5 | 800.267 | 796.098 | 0.99981405 | 0.988001 | 1.007288 | 1.05 | 通过 |
| f32 | 768 | qkv | 12 | 1905.940 | 1908.428 | 1.00140402 | 0.993488 | 1.035319 | 1.05 | 通过 |
| f32 | 768 | proj | 1 | 52.051 | 52.003 | 0.99956608 | 0.990516 | 1.007617 | 1.05 | 通过 |
| f32 | 768 | proj | 2 | 103.933 | 103.928 | 1.00067092 | 0.992270 | 1.016491 | 1.05 | 通过 |
| f32 | 768 | proj | 3 | 155.925 | 155.910 | 1.00054317 | 0.995495 | 1.015312 | 1.05 | 通过 |
| f32 | 768 | proj | 4 | 207.975 | 207.792 | 0.99965223 | 0.995873 | 1.004694 | 1.05 | 通过 |
| f32 | 768 | proj | 5 | 267.866 | 267.037 | 0.99690600 | 0.953230 | 1.019289 | 1.05 | 通过 |
| f32 | 768 | proj | 12 | 673.245 | 673.920 | 1.00106674 | 0.964023 | 1.005921 | 1.05 | 通过 |
| f32 | 768 | fc | 1 | 237.799 | 239.605 | 1.00667196 | 0.704666 | 1.054397 | 1.05 | 通过 |
| f32 | 768 | fc | 2 | 470.827 | 469.258 | 1.00188260 | 0.984797 | 1.005969 | 1.05 | 通过 |
| f32 | 768 | fc | 3 | 694.696 | 695.444 | 0.99949843 | 0.988267 | 1.012362 | 1.05 | 通过 |
| f32 | 768 | fc | 4 | 908.609 | 907.180 | 1.00067761 | 0.988170 | 1.008971 | 1.05 | 通过 |
| f32 | 768 | fc | 5 | 1151.080 | 1146.946 | 0.99930239 | 0.989246 | 1.008622 | 1.05 | 通过 |
| f32 | 768 | fc | 12 | 2796.280 | 2794.171 | 0.99756745 | 0.989187 | 1.006588 | 1.05 | 通过 |
| f32 | 768 | out | 1 | 274.254 | 274.313 | 1.00008413 | 0.987324 | 1.013689 | 1.05 | 通过 |
| f32 | 768 | out | 2 | 563.380 | 570.474 | 1.00134332 | 0.887771 | 1.077164 | 1.05 | 通过 |
| f32 | 768 | out | 3 | 823.585 | 820.809 | 1.00114652 | 0.983026 | 1.024559 | 1.05 | 通过 |
| f32 | 768 | out | 4 | 1088.604 | 1092.919 | 1.00368291 | 0.989471 | 1.010965 | 1.05 | 通过 |
| f32 | 768 | out | 5 | 1366.999 | 1357.863 | 0.99471340 | 0.975822 | 1.005224 | 1.05 | 通过 |
| f32 | 768 | out | 12 | 3187.768 | 3198.539 | 1.00054895 | 0.981807 | 1.013237 | 1.05 | 通过 |

### v3 全表

| 精度 | 基宽 | 投影 | 行数 | old_us | new_us | ratio | p10 | p90 | budget | 结果 |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| i8 | 192 | qkv | 1 | 11.159 | 11.140 | 0.99750172 | 0.988844 | 1.057963 | 1.05 | 通过 |
| i8 | 192 | qkv | 2 | 22.356 | 62.123 | 2.78635885 | 2.750075 | 2.940452 | 0.95 | 失败 |
| i8 | 192 | qkv | 3 | 33.207 | 72.189 | 2.17917924 | 2.158772 | 2.214598 | 0.95 | 失败 |
| i8 | 192 | qkv | 4 | 43.821 | 122.130 | 2.78914751 | 2.768945 | 2.831854 | 0.95 | 失败 |
| i8 | 192 | qkv | 5 | 54.419 | 133.118 | 2.44371728 | 2.406809 | 2.465876 | 0.95 | 失败 |
| i8 | 192 | qkv | 12 | 131.434 | 366.365 | 2.79652741 | 2.753499 | 2.828368 | 0.95 | 失败 |
| i8 | 192 | proj | 1 | 3.656 | 3.648 | 0.99575446 | 0.985957 | 1.010376 | 1.05 | 通过 |
| i8 | 192 | proj | 2 | 7.320 | 20.363 | 2.78458952 | 2.761026 | 2.837027 | 0.95 | 失败 |
| i8 | 192 | proj | 3 | 10.973 | 24.084 | 2.19250241 | 2.172940 | 2.214068 | 0.95 | 失败 |
| i8 | 192 | proj | 4 | 14.510 | 40.640 | 2.79507017 | 2.776276 | 2.841566 | 0.95 | 失败 |
| i8 | 192 | proj | 5 | 18.152 | 44.284 | 2.44208166 | 2.409835 | 2.458720 | 0.95 | 失败 |
| i8 | 192 | proj | 12 | 43.524 | 121.921 | 2.79872582 | 2.779805 | 2.856975 | 0.95 | 失败 |
| i8 | 192 | fc | 1 | 14.524 | 14.580 | 1.00165997 | 0.990552 | 1.012087 | 1.05 | 通过 |
| i8 | 192 | fc | 2 | 29.130 | 81.187 | 2.79681269 | 2.752669 | 2.817609 | 0.95 | 失败 |
| i8 | 192 | fc | 3 | 43.860 | 95.799 | 2.19244123 | 2.065917 | 2.207254 | 0.95 | 失败 |
| i8 | 192 | fc | 4 | 106.773 | 337.282 | 3.35173007 | 1.430159 | 5.994973 | 0.95 | 失败 |
| i8 | 192 | fc | 5 | 116.817 | 439.671 | 3.17029494 | 0.818403 | 7.008877 | 0.95 | 失败 |
| i8 | 192 | fc | 12 | 180.850 | 499.170 | 2.77381966 | 2.293539 | 5.785061 | 0.95 | 失败 |
| i8 | 192 | out | 1 | 15.385 | 15.378 | 0.99994247 | 0.988391 | 1.013212 | 1.05 | 通过 |
| i8 | 192 | out | 2 | 30.751 | 79.414 | 2.58882360 | 2.561060 | 2.597030 | 0.95 | 失败 |
| i8 | 192 | out | 3 | 46.044 | 95.078 | 2.06707062 | 2.037894 | 2.082708 | 0.95 | 失败 |
| i8 | 192 | out | 4 | 61.411 | 158.933 | 2.58876897 | 2.562307 | 2.611248 | 0.95 | 失败 |
| i8 | 192 | out | 5 | 76.548 | 173.948 | 2.27699654 | 2.262395 | 2.285551 | 0.95 | 失败 |
| i8 | 192 | out | 12 | 184.025 | 478.690 | 2.59058640 | 2.567271 | 2.619459 | 0.95 | 失败 |
| i8 | 768 | qkv | 1 | 190.927 | 191.931 | 1.00544689 | 0.824541 | 1.295328 | 1.05 | 通过 |
| i8 | 768 | qkv | 2 | 379.030 | 974.809 | 2.57527295 | 2.538868 | 2.605338 | 0.95 | 失败 |
| i8 | 768 | qkv | 3 | 555.213 | 1142.455 | 2.06008393 | 2.045641 | 2.077434 | 0.95 | 失败 |
| i8 | 768 | qkv | 4 | 764.855 | 2227.766 | 2.58856664 | 1.869385 | 4.533910 | 0.95 | 失败 |
| i8 | 768 | qkv | 5 | 1404.796 | 3726.655 | 2.37860984 | 1.571750 | 4.087430 | 0.95 | 失败 |
| i8 | 768 | qkv | 12 | 3128.646 | 7979.964 | 2.58755480 | 1.346804 | 4.616993 | 0.95 | 失败 |
| i8 | 768 | proj | 1 | 96.912 | 98.550 | 0.94378549 | 0.472321 | 2.343734 | 1.05 | 通过 |
| i8 | 768 | proj | 2 | 190.348 | 495.981 | 2.22907434 | 1.110807 | 4.441806 | 0.95 | 失败 |
| i8 | 768 | proj | 3 | 189.259 | 418.383 | 2.06846494 | 1.848222 | 2.558946 | 0.95 | 失败 |
| i8 | 768 | proj | 4 | 246.668 | 640.671 | 2.59294594 | 2.569543 | 2.616916 | 0.95 | 失败 |
| i8 | 768 | proj | 5 | 306.431 | 695.106 | 2.27694173 | 2.248241 | 2.288079 | 0.95 | 失败 |
| i8 | 768 | proj | 12 | 735.170 | 1909.593 | 2.59776790 | 2.580012 | 2.626225 | 0.95 | 失败 |
| i8 | 768 | fc | 1 | 246.935 | 246.932 | 1.00048979 | 0.959026 | 1.014372 | 1.05 | 通过 |
| i8 | 768 | fc | 2 | 489.985 | 1266.583 | 2.60275697 | 2.405906 | 3.431560 | 0.95 | 失败 |
| i8 | 768 | fc | 3 | 984.887 | 2117.365 | 2.03757026 | 1.393916 | 3.702791 | 0.95 | 失败 |
| i8 | 768 | fc | 4 | 1014.732 | 3029.474 | 2.59228712 | 2.560263 | 3.811479 | 0.95 | 失败 |
| i8 | 768 | fc | 5 | 1272.385 | 2894.253 | 2.26989796 | 2.243129 | 2.367499 | 0.95 | 失败 |
| i8 | 768 | fc | 12 | 3032.243 | 7841.599 | 2.58854772 | 2.566145 | 3.002630 | 0.95 | 失败 |
| i8 | 768 | out | 1 | 273.957 | 274.251 | 1.00051192 | 0.991433 | 1.006964 | 1.05 | 通过 |
| i8 | 768 | out | 2 | 547.668 | 1260.698 | 2.30144764 | 2.283924 | 2.315992 | 0.95 | 失败 |
| i8 | 768 | out | 3 | 830.938 | 1557.758 | 1.87264810 | 1.858976 | 1.886252 | 0.95 | 失败 |
| i8 | 768 | out | 4 | 1114.717 | 2554.893 | 2.30574865 | 2.274621 | 2.323506 | 0.95 | 失败 |
| i8 | 768 | out | 5 | 1381.889 | 2835.249 | 2.05260492 | 2.035340 | 2.066024 | 0.95 | 失败 |
| i8 | 768 | out | 12 | 3203.695 | 7363.719 | 2.29763484 | 2.270828 | 2.325862 | 0.95 | 失败 |
| f32 | 192 | qkv | 1 | 8.611 | 8.594 | 1.00205318 | 0.994864 | 1.020353 | 1.05 | 通过 |
| f32 | 192 | qkv | 2 | 17.285 | 17.448 | 1.00832307 | 0.995992 | 1.027702 | 1.05 | 通过 |
| f32 | 192 | qkv | 3 | 25.943 | 26.124 | 1.00709022 | 1.004030 | 1.025612 | 1.05 | 通过 |
| f32 | 192 | qkv | 4 | 34.535 | 34.830 | 1.00852354 | 0.992740 | 1.023504 | 1.05 | 通过 |
| f32 | 192 | qkv | 5 | 43.183 | 43.471 | 1.00715452 | 1.003252 | 1.021428 | 1.05 | 通过 |
| f32 | 192 | qkv | 12 | 103.616 | 104.388 | 1.00627754 | 0.981607 | 1.026071 | 1.05 | 通过 |
| f32 | 192 | proj | 1 | 2.932 | 2.955 | 1.00834589 | 1.002746 | 1.023143 | 1.05 | 通过 |
| f32 | 192 | proj | 2 | 5.929 | 5.989 | 1.01008430 | 1.003197 | 1.022189 | 1.05 | 通过 |
| f32 | 192 | proj | 3 | 8.892 | 8.945 | 1.00672082 | 0.989655 | 1.009334 | 1.05 | 通过 |
| f32 | 192 | proj | 4 | 11.777 | 11.880 | 1.00776573 | 0.997262 | 1.020492 | 1.05 | 通过 |
| f32 | 192 | proj | 5 | 14.721 | 14.918 | 1.00852542 | 0.998196 | 1.027940 | 1.05 | 通过 |
| f32 | 192 | proj | 12 | 35.259 | 35.503 | 1.00719502 | 1.000176 | 1.019694 | 1.05 | 通过 |
| f32 | 192 | fc | 1 | 11.815 | 11.942 | 1.00022481 | 0.944492 | 1.110261 | 1.05 | 通过 |
| f32 | 192 | fc | 2 | 23.585 | 23.308 | 1.00709929 | 1.000368 | 1.015442 | 1.05 | 通过 |
| f32 | 192 | fc | 3 | 34.406 | 34.688 | 1.00767793 | 0.998301 | 1.023989 | 1.05 | 通过 |
| f32 | 192 | fc | 4 | 47.406 | 47.950 | 1.00547991 | 0.998062 | 1.018556 | 1.05 | 通过 |
| f32 | 192 | fc | 5 | 57.380 | 57.780 | 1.00689191 | 0.992681 | 1.009304 | 1.05 | 通过 |
| f32 | 192 | fc | 12 | 138.279 | 139.003 | 1.00692143 | 0.988287 | 1.009898 | 1.05 | 通过 |
| f32 | 192 | out | 1 | 13.012 | 13.016 | 1.00063948 | 0.991356 | 1.002503 | 1.05 | 通过 |
| f32 | 192 | out | 2 | 26.044 | 26.079 | 1.00132665 | 0.998687 | 1.007052 | 1.05 | 通过 |
| f32 | 192 | out | 3 | 39.042 | 39.118 | 1.00201373 | 0.980923 | 1.011200 | 1.05 | 通过 |
| f32 | 192 | out | 4 | 52.172 | 52.257 | 1.00147757 | 0.996731 | 1.014301 | 1.05 | 通过 |
| f32 | 192 | out | 5 | 65.004 | 65.034 | 1.00042425 | 0.999023 | 1.015409 | 1.05 | 通过 |
| f32 | 192 | out | 12 | 156.152 | 156.548 | 1.00102605 | 0.990794 | 1.017301 | 1.05 | 通过 |
| f32 | 768 | qkv | 1 | 157.028 | 157.318 | 1.00158614 | 0.984477 | 1.014671 | 1.05 | 通过 |
| f32 | 768 | qkv | 2 | 319.444 | 314.145 | 0.98273340 | 0.962164 | 1.009970 | 1.05 | 通过 |
| f32 | 768 | qkv | 3 | 471.788 | 472.396 | 1.00187221 | 0.986552 | 1.014190 | 1.05 | 通过 |
| f32 | 768 | qkv | 4 | 636.411 | 639.255 | 1.00437940 | 0.998935 | 1.029508 | 1.05 | 通过 |
| f32 | 768 | qkv | 5 | 790.220 | 791.625 | 1.00292461 | 0.993490 | 1.022345 | 1.05 | 通过 |
| f32 | 768 | qkv | 12 | 1888.749 | 1889.961 | 1.00146904 | 0.962820 | 1.008543 | 1.05 | 通过 |
| f32 | 768 | proj | 1 | 52.006 | 52.090 | 1.00195598 | 0.996228 | 1.019651 | 1.05 | 通过 |
| f32 | 768 | proj | 2 | 103.889 | 104.067 | 1.00092749 | 0.991267 | 1.013716 | 1.05 | 通过 |
| f32 | 768 | proj | 3 | 155.684 | 155.874 | 1.00066694 | 0.985985 | 1.006699 | 1.05 | 通过 |
| f32 | 768 | proj | 4 | 207.676 | 207.982 | 1.00088406 | 0.988454 | 1.013429 | 1.05 | 通过 |
| f32 | 768 | proj | 5 | 259.635 | 260.140 | 1.00151739 | 0.998901 | 1.013198 | 1.05 | 通过 |
| f32 | 768 | proj | 12 | 624.752 | 625.213 | 1.00130801 | 0.995078 | 1.014598 | 1.05 | 通过 |
| f32 | 768 | fc | 1 | 217.537 | 215.874 | 0.99874647 | 0.980116 | 1.024155 | 1.05 | 通过 |
| f32 | 768 | fc | 2 | 429.695 | 430.633 | 1.00249044 | 0.995946 | 1.010925 | 1.05 | 通过 |
| f32 | 768 | fc | 3 | 643.480 | 645.971 | 1.00212482 | 0.990603 | 1.017556 | 1.05 | 通过 |
| f32 | 768 | fc | 4 | 864.721 | 866.073 | 1.00076777 | 0.997252 | 1.003327 | 1.05 | 通过 |
| f32 | 768 | fc | 5 | 1074.185 | 1078.121 | 1.00176210 | 0.971356 | 1.018025 | 1.05 | 通过 |
| f32 | 768 | fc | 12 | 2569.753 | 2578.673 | 1.00208099 | 0.998035 | 1.024778 | 1.05 | 通过 |
| f32 | 768 | out | 1 | 271.255 | 270.361 | 1.00082990 | 0.981759 | 1.069573 | 1.05 | 通过 |
| f32 | 768 | out | 2 | 536.949 | 539.661 | 1.00212942 | 0.986538 | 1.015022 | 1.05 | 通过 |
| f32 | 768 | out | 3 | 783.174 | 778.623 | 0.99819806 | 0.983702 | 1.002189 | 1.05 | 通过 |
| f32 | 768 | out | 4 | 1039.162 | 1038.762 | 1.00039255 | 0.988145 | 1.003372 | 1.05 | 通过 |
| f32 | 768 | out | 5 | 1295.859 | 1297.292 | 0.99949258 | 0.979567 | 1.018078 | 1.05 | 通过 |
| f32 | 768 | out | 12 | 3131.085 | 3131.417 | 0.99935796 | 0.980945 | 1.012462 | 1.05 | 通过 |

## Reproduction

以下 patch 和测试源是本次实验的完整快照，不是产品实现。应在仓库外新建固定基线 `6acc7839572fb7df221a8e4910467c9dedd72fb8` 的独立 worktree 后操作，不在共享主检出或正在开发的目录执行。三个 diff 均以同一基线为起点，不能叠加。构建目录使用默认 `reference/target/`。每一轮先构建完再计时，期间不要运行自己的其它构建；另外按每秒进程名采样记录外部构建观察，不能因门禁失败擅自停止其它会话。

从本篇按顺序提取第三个 diff 块到 `prototype.patch`，唯一 rust 块到 `reference/src/linear_paired_inputs_tests.rs`。先只在基线 lib.rs 末尾增加 `#[cfg(test)] mod linear_paired_inputs_tests;` 注册，构建并跑 red；然后将 lib.rs 恢复到固定基线，应用第三个完整 diff，再按相同命令跑 v3。完整 diff 已含测试注册，不再手动增加第二次。临时提取文件放 worktree 外，结束后恢复基线并删临时测试。

```sh
cargo test --manifest-path reference/Cargo.toml --release --locked linear_paired_inputs_tests -- --skip complete_linear_paired_budget --nocapture
cargo test --manifest-path reference/Cargo.toml --release --locked linear_paired_inputs_tests::complete_linear_paired_budget -- --exact --ignored --nocapture --test-threads=1
```

red 计时夹具与附录完全相同；有限舍入补强在 red binary 编译后、生产修改前加入，所以 red 预算 binary 未包含这项新正确性测试，不影响同一预算路径。机器码检查使用 `xcrun llvm-nm <测试 binary>` 查找符号，再以 `xcrun llvm-objdump --disassemble-symbols=<完整符号> <测试 binary>` 输出；v1/v2 看 linear_into 的新增配对内环，v3 看独立 dot_i8_pair。

v1/v2 不必补测寻找更好数字；仅用于复核本文机器码现象，未计时事实保持不变。

### v1 完整生产与注册 diff

```diff
diff --git a/reference/src/lib.rs b/reference/src/lib.rs
index 69340bf..3d5003a 100644
--- a/reference/src/lib.rs
+++ b/reference/src/lib.rs
@@ -1240,7 +1240,27 @@ fn linear_into(
     out: &mut [f32],
 ) {
     let rows = x.len() / inputs;
-    for row in 0..rows {
+    let mut first_remaining = 0;
+    if rows >= 2 {
+        if let Tensor::Quantized { values, scales } = &w.inner {
+            for pair in 0..rows / 2 {
+                let row = pair * 2;
+                let a = &x[row * inputs..(row + 1) * inputs];
+                let b = &x[(row + 1) * inputs..(row + 2) * inputs];
+                let (before, after) = out.split_at_mut((row + 1) * outputs);
+                let target_a = &mut before[row * outputs..];
+                let target_b = &mut after[..outputs];
+                for index in 0..outputs {
+                    let weight = &values[index * inputs..(index + 1) * inputs];
+                    let (left, right) = dot_i8_pair(a, b, weight);
+                    target_a[index] = left * scales[index] + bias[index];
+                    target_b[index] = right * scales[index] + bias[index];
+                }
+            }
+            first_remaining = rows / 2 * 2;
+        }
+    }
+    for row in first_remaining..rows {
         let source = &x[row * inputs..(row + 1) * inputs];
         let target = &mut out[row * outputs..(row + 1) * outputs];
         for (index, slot) in target.iter_mut().enumerate() {
@@ -1270,6 +1290,32 @@ fn dot_i8(a: &[f32], b: &[i8]) -> f32 {
     total
 }

+/// 两个输入位置共享权重转换，各自保留原八部分和及归约顺序。
+fn dot_i8_pair(a: &[f32], b: &[f32], weights: &[i8]) -> (f32, f32) {
+    const LANES: usize = 8;
+    let mut partial_a = [0.0f32; LANES];
+    let mut partial_b = [0.0f32; LANES];
+    let left = a.chunks_exact(LANES);
+    let right = b.chunks_exact(LANES);
+    let quantized = weights.chunks_exact(LANES);
+    let tails = (left.remainder(), right.remainder(), quantized.remainder());
+    for ((x, y), w) in left.zip(right).zip(quantized) {
+        for lane in 0..LANES {
+            let weight = f32::from(w[lane]);
+            partial_a[lane] += x[lane] * weight;
+            partial_b[lane] += y[lane] * weight;
+        }
+    }
+    let mut total_a = partial_a.iter().sum::<f32>();
+    let mut total_b = partial_b.iter().sum::<f32>();
+    for ((x, y), w) in tails.0.iter().zip(tails.1).zip(tails.2) {
+        let weight = f32::from(*w);
+        total_a += x * weight;
+        total_b += y * weight;
+    }
+    (total_a, total_b)
+}
+
 fn layer_norm(x: &[f32], weight: &[f32], bias: &[f32], width: usize) -> Vec<f32> {
     let mut out = vec![0.0; x.len()];
     layer_norm_into(x, weight, bias, width, &mut out);
@@ -1930,3 +1976,6 @@ mod prefix_final_row_tests;

 #[cfg(test)]
 mod terminal_prefix_tests;
+
+#[cfg(test)]
+mod linear_paired_inputs_tests;
```

### v2 完整生产与注册 diff

```diff
diff --git a/reference/src/lib.rs b/reference/src/lib.rs
index 69340bf..651db1d 100644
--- a/reference/src/lib.rs
+++ b/reference/src/lib.rs
@@ -1240,7 +1240,27 @@ fn linear_into(
     out: &mut [f32],
 ) {
     let rows = x.len() / inputs;
-    for row in 0..rows {
+    let mut first_remaining = 0;
+    if rows >= 2 {
+        if let Tensor::Quantized { values, scales } = &w.inner {
+            for pair in 0..rows / 2 {
+                let row = pair * 2;
+                let a = &x[row * inputs..(row + 1) * inputs];
+                let b = &x[(row + 1) * inputs..(row + 2) * inputs];
+                let (before, after) = out.split_at_mut((row + 1) * outputs);
+                let target_a = &mut before[row * outputs..];
+                let target_b = &mut after[..outputs];
+                for index in 0..outputs {
+                    let weight = &values[index * inputs..(index + 1) * inputs];
+                    let (left, right) = dot_i8_pair(a, b, weight);
+                    target_a[index] = left * scales[index] + bias[index];
+                    target_b[index] = right * scales[index] + bias[index];
+                }
+            }
+            first_remaining = rows / 2 * 2;
+        }
+    }
+    for row in first_remaining..rows {
         let source = &x[row * inputs..(row + 1) * inputs];
         let target = &mut out[row * outputs..(row + 1) * outputs];
         for (index, slot) in target.iter_mut().enumerate() {
@@ -1270,6 +1290,34 @@ fn dot_i8(a: &[f32], b: &[i8]) -> f32 {
     total
 }

+/// 两个输入位置共享权重转换，各自保留原八部分和及归约顺序。
+fn dot_i8_pair(a: &[f32], b: &[f32], weights: &[i8]) -> (f32, f32) {
+    const LANES: usize = 8;
+    let mut partial_a = [0.0f32; LANES];
+    let mut partial_b = [0.0f32; LANES];
+    let left = a.chunks_exact(LANES);
+    let right = b.chunks_exact(LANES);
+    let quantized = weights.chunks_exact(LANES);
+    let tails = (left.remainder(), right.remainder(), quantized.remainder());
+    for ((x, y), w) in left.zip(right).zip(quantized) {
+        let converted = std::array::from_fn::<_, LANES, _>(|lane| f32::from(w[lane]));
+        for lane in 0..LANES {
+            partial_a[lane] += x[lane] * converted[lane];
+        }
+        for lane in 0..LANES {
+            partial_b[lane] += y[lane] * converted[lane];
+        }
+    }
+    let mut total_a = partial_a.iter().sum::<f32>();
+    let mut total_b = partial_b.iter().sum::<f32>();
+    for ((x, y), w) in tails.0.iter().zip(tails.1).zip(tails.2) {
+        let weight = f32::from(*w);
+        total_a += x * weight;
+        total_b += y * weight;
+    }
+    (total_a, total_b)
+}
+
 fn layer_norm(x: &[f32], weight: &[f32], bias: &[f32], width: usize) -> Vec<f32> {
     let mut out = vec![0.0; x.len()];
     layer_norm_into(x, weight, bias, width, &mut out);
@@ -1930,3 +1978,6 @@ mod prefix_final_row_tests;

 #[cfg(test)]
 mod terminal_prefix_tests;
+
+#[cfg(test)]
+mod linear_paired_inputs_tests;
```

### v3 完整生产与注册 diff

```diff
diff --git a/reference/src/lib.rs b/reference/src/lib.rs
index 69340bf..a30bf40 100644
--- a/reference/src/lib.rs
+++ b/reference/src/lib.rs
@@ -1240,7 +1240,27 @@ fn linear_into(
     out: &mut [f32],
 ) {
     let rows = x.len() / inputs;
-    for row in 0..rows {
+    let mut first_remaining = 0;
+    if rows >= 2 {
+        if let Tensor::Quantized { values, scales } = &w.inner {
+            for pair in 0..rows / 2 {
+                let row = pair * 2;
+                let a = &x[row * inputs..(row + 1) * inputs];
+                let b = &x[(row + 1) * inputs..(row + 2) * inputs];
+                let (before, after) = out.split_at_mut((row + 1) * outputs);
+                let target_a = &mut before[row * outputs..];
+                let target_b = &mut after[..outputs];
+                for index in 0..outputs {
+                    let weight = &values[index * inputs..(index + 1) * inputs];
+                    let (left, right) = dot_i8_pair(a, b, weight);
+                    target_a[index] = left * scales[index] + bias[index];
+                    target_b[index] = right * scales[index] + bias[index];
+                }
+            }
+            first_remaining = rows / 2 * 2;
+        }
+    }
+    for row in first_remaining..rows {
         let source = &x[row * inputs..(row + 1) * inputs];
         let target = &mut out[row * outputs..(row + 1) * outputs];
         for (index, slot) in target.iter_mut().enumerate() {
@@ -1270,6 +1290,35 @@ fn dot_i8(a: &[f32], b: &[i8]) -> f32 {
     total
 }

+/// 两个输入位置共享权重转换，各自保留原八部分和及归约顺序。
+#[inline(never)]
+fn dot_i8_pair(a: &[f32], b: &[f32], weights: &[i8]) -> (f32, f32) {
+    const LANES: usize = 8;
+    let mut partial_a = [0.0f32; LANES];
+    let mut partial_b = [0.0f32; LANES];
+    let left = a.chunks_exact(LANES);
+    let right = b.chunks_exact(LANES);
+    let quantized = weights.chunks_exact(LANES);
+    let tails = (left.remainder(), right.remainder(), quantized.remainder());
+    for ((x, y), w) in left.zip(right).zip(quantized) {
+        let converted = std::array::from_fn::<_, LANES, _>(|lane| f32::from(w[lane]));
+        for lane in 0..LANES {
+            partial_a[lane] += x[lane] * converted[lane];
+        }
+        for lane in 0..LANES {
+            partial_b[lane] += y[lane] * converted[lane];
+        }
+    }
+    let mut total_a = partial_a.iter().sum::<f32>();
+    let mut total_b = partial_b.iter().sum::<f32>();
+    for ((x, y), w) in tails.0.iter().zip(tails.1).zip(tails.2) {
+        let weight = f32::from(*w);
+        total_a += x * weight;
+        total_b += y * weight;
+    }
+    (total_a, total_b)
+}
+
 fn layer_norm(x: &[f32], weight: &[f32], bias: &[f32], width: usize) -> Vec<f32> {
     let mut out = vec![0.0; x.len()];
     layer_norm_into(x, weight, bias, width, &mut out);
@@ -1930,3 +1979,6 @@ mod prefix_final_row_tests;

 #[cfg(test)]
 mod terminal_prefix_tests;
+
+#[cfg(test)]
+mod linear_paired_inputs_tests;
```

### 完整临时测试

```rust
use super::*;
use std::hint::black_box;
use std::time::Instant;

// 冻结 6acc7839572fb7df221a8e4910467c9dedd72fb8 的完整线性层与点积，不借用生产 helper。
fn frozen_dot(a: &[f32], b: &[f32]) -> f32 {
    const LANES: usize = 8;
    let mut partial = [0.0f32; LANES];
    let left = a.chunks_exact(LANES);
    let right = b.chunks_exact(LANES);
    let (tail_left, tail_right) = (left.remainder(), right.remainder());
    for (x, y) in left.zip(right) {
        for lane in 0..LANES {
            partial[lane] += x[lane] * y[lane];
        }
    }
    let mut total = partial.iter().sum::<f32>();
    for (x, y) in tail_left.iter().zip(tail_right) {
        total += x * y;
    }
    total
}

fn frozen_linear(x: &[f32], w: &Matrix, bias: &[f32], inputs: usize, outputs: usize) -> Vec<f32> {
    let mut out = vec![0.0; x.len() / inputs * outputs];
    frozen_linear_into(x, w, bias, inputs, outputs, &mut out);
    out
}

fn frozen_linear_into(
    x: &[f32],
    w: &Matrix,
    bias: &[f32],
    inputs: usize,
    outputs: usize,
    out: &mut [f32],
) {
    let rows = x.len() / inputs;
    for row in 0..rows {
        let source = &x[row * inputs..(row + 1) * inputs];
        let target = &mut out[row * outputs..(row + 1) * outputs];
        for (index, slot) in target.iter_mut().enumerate() {
            *slot = frozen_dot_row(w, source, index, inputs) + bias[index];
        }
    }
}

fn frozen_dot_i8(a: &[f32], b: &[i8]) -> f32 {
    const LANES: usize = 8;
    let mut partial = [0.0f32; LANES];
    let left = a.chunks_exact(LANES);
    let right = b.chunks_exact(LANES);
    let (tail_left, tail_right) = (left.remainder(), right.remainder());
    for (x, y) in left.zip(right) {
        for lane in 0..LANES {
            partial[lane] += x[lane] * f32::from(y[lane]);
        }
    }
    let mut total = partial.iter().sum::<f32>();
    for (x, y) in tail_left.iter().zip(tail_right) {
        total += x * f32::from(*y);
    }
    total
}

fn frozen_dot_row(w: &Matrix, x: &[f32], row: usize, width: usize) -> f32 {
    let span = row * width..(row + 1) * width;
    match &w.inner {
        Tensor::Float(values) => frozen_dot(x, &values[span]),
        Tensor::Quantized { values, scales } => frozen_dot_i8(x, &values[span]) * scales[row],
    }
}

fn matrix(inputs: usize, outputs: usize, quantized: bool) -> Matrix {
    Matrix {
        inner: if quantized {
            Tensor::Quantized {
                values: (0..inputs * outputs)
                    .map(|i| ((i * 7 % 251) as i16 - 125) as i8)
                    .collect(),
                scales: (0..outputs).map(|i| (i % 7 + 1) as f32 / 128.0).collect(),
            }
        } else {
            Tensor::Float(
                (0..inputs * outputs)
                    .map(|i| (i as f32 % 23.0 - 11.0) / 32.0)
                    .collect(),
            )
        },
    }
}

fn input(count: usize) -> Vec<f32> {
    (0..count)
        .map(|i| {
            if i % 11 == 0 {
                -0.0
            } else {
                ((i * 11 % 31) as f32 - 15.0) / 16.0
            }
        })
        .collect()
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn complete_linear_matches_frozen_bits() {
    for quantized in [false, true] {
        for inputs in [1, 7, 8, 9, 192, 768] {
            for outputs in [0, 1, 3, 4] {
                let mut w = matrix(inputs, outputs, quantized);
                let mut bias = input(outputs);
                for exceptional in [false, true] {
                    if exceptional {
                        if let Tensor::Float(values) = &mut w.inner {
                            if !values.is_empty() {
                                values[0] = f32::NEG_INFINITY;
                            }
                        }
                        if let Tensor::Quantized { scales, .. } = &mut w.inner {
                            if !scales.is_empty() {
                                scales[0] = f32::INFINITY;
                            }
                        }
                        if outputs > 1 {
                            bias[1] = f32::from_bits(0x7fc0_0023);
                        }
                    }
                    for rows in [0, 1, 2, 3, 4, 5, 12] {
                        for trailing in [0, inputs - 1] {
                            let mut x = input(rows * inputs + trailing);
                            if exceptional && rows > 0 {
                                x[0] = f32::from_bits(0x7fc0_0017);
                                if x.len() > 2 {
                                    x[2] = f32::INFINITY;
                                }
                            }
                            let old = frozen_linear(&x, &w, &bias, inputs, outputs);
                            let new = linear(&x, &w, &bias, inputs, outputs);
                            assert_eq!(bits(&old), bits(&new), "quantized={quantized} inputs={inputs} outputs={outputs} rows={rows} trailing={trailing} exceptional={exceptional}");
                            let mut actual = vec![123.0; rows * outputs + 3];
                            let mut expected = actual.clone();
                            frozen_linear_into(&x, &w, &bias, inputs, outputs, &mut expected);
                            linear_into(&x, &w, &bias, inputs, outputs, &mut actual);
                            assert_eq!(bits(&expected), bits(&actual), "写入与尾部哨兵不一致");
                        }
                    }
                }
            }
        }
    }
}

// 非整数尾数、宽指数与正负相消，确保有限值用例能识别舍入顺序变化。
fn rounding_values(count: usize, salt: u32) -> Vec<f32> {
    let mut state = salt;
    (0..count)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let exponent = 112 + ((state >> 23) % 27);
            let sign = if i % 2 == 0 { 0 } else { 1 << 31 };
            f32::from_bits(sign | (exponent << 23) | (state & 0x007f_ffff))
        })
        .collect()
}

#[test]
fn finite_rounding_matches_frozen_bits() {
    for quantized in [false, true] {
        for inputs in [1, 7, 8, 9, 192, 768] {
            for outputs in [1, 3, 4] {
                let mut w = matrix(inputs, outputs, quantized);
                match &mut w.inner {
                    Tensor::Float(values) => {
                        *values = rounding_values(inputs * outputs, 0x91fe_a731)
                    }
                    Tensor::Quantized { scales, .. } => {
                        *scales = (0..outputs).map(|i| (i as f32 + 1.0) / 1301.0).collect();
                    }
                }
                let bias = rounding_values(outputs, 0x32ae_f17b);
                for rows in [0, 1, 2, 3, 4, 5, 12] {
                    for trailing in [0, inputs - 1] {
                        let x = rounding_values(rows * inputs + trailing, 0x817a_e9d5);
                        let old = frozen_linear(&x, &w, &bias, inputs, outputs);
                        let new = linear(&x, &w, &bias, inputs, outputs);
                        assert_eq!(bits(&old), bits(&new), "有限值舍入 quantized={quantized} inputs={inputs} outputs={outputs} rows={rows} trailing={trailing}");
                        let mut actual = vec![123.0; rows * outputs + 3];
                        let mut expected = actual.clone();
                        frozen_linear_into(&x, &w, &bias, inputs, outputs, &mut expected);
                        linear_into(&x, &w, &bias, inputs, outputs, &mut actual);
                        assert_eq!(bits(&expected), bits(&actual));
                    }
                }
            }
        }
    }
}

#[inline(never)]
fn old_linear(x: &[f32], w: &Matrix, bias: &[f32], inputs: usize, outputs: usize) -> Vec<f32> {
    frozen_linear(x, w, bias, inputs, outputs)
}

#[inline(never)]
fn new_linear(x: &[f32], w: &Matrix, bias: &[f32], inputs: usize, outputs: usize) -> Vec<f32> {
    linear(x, w, bias, inputs, outputs)
}

type Linear = fn(&[f32], &Matrix, &[f32], usize, usize) -> Vec<f32>;
fn elapsed(
    f: Linear,
    x: &[f32],
    w: &Matrix,
    bias: &[f32],
    inputs: usize,
    outputs: usize,
    repeats: usize,
) -> f64 {
    let start = Instant::now();
    for _ in 0..repeats {
        drop(black_box(f(
            black_box(x),
            black_box(w),
            black_box(bias),
            black_box(inputs),
            black_box(outputs),
        )));
    }
    start.elapsed().as_secs_f64()
}

#[test]
#[ignore = "独占运行完整线性层 96 形状配对预算，包含申请和析构"]
fn complete_linear_paired_budget() {
    assert!(!cfg!(debug_assertions), "计时必须使用 release");
    let mut failures = Vec::new();
    for quantized in [true, false] {
        for base in [192, 768] {
            for (name, inputs, outputs) in [
                ("qkv", base, 3 * base),
                ("proj", base, base),
                ("fc", base, 4 * base),
                ("out", 4 * base, base),
            ] {
                let w = matrix(inputs, outputs, quantized);
                let bias = input(outputs);
                for rows in [1, 2, 3, 4, 5, 12] {
                    let x = input(rows * inputs);
                    assert_eq!(
                        bits(&old_linear(&x, &w, &bias, inputs, outputs)),
                        bits(&new_linear(&x, &w, &bias, inputs, outputs))
                    );
                    let mut repeats = 1;
                    loop {
                        let old = elapsed(old_linear, &x, &w, &bias, inputs, outputs, repeats);
                        let new = elapsed(new_linear, &x, &w, &bias, inputs, outputs, repeats);
                        if old.min(new) >= 0.050 {
                            break;
                        }
                        repeats *= 2;
                    }
                    let mut ratios = Vec::new();
                    for pair in 0..21 {
                        let (old, new) = if pair % 2 == 0 {
                            let old = elapsed(old_linear, &x, &w, &bias, inputs, outputs, repeats);
                            (
                                old,
                                elapsed(new_linear, &x, &w, &bias, inputs, outputs, repeats),
                            )
                        } else {
                            let new = elapsed(new_linear, &x, &w, &bias, inputs, outputs, repeats);
                            (
                                elapsed(old_linear, &x, &w, &bias, inputs, outputs, repeats),
                                new,
                            )
                        };
                        println!("sample quantized={quantized} base={base} kind={name} rows={rows} repeats={repeats} pair={pair} old_s={old:.9} new_s={new:.9}");
                        ratios.push(new / old);
                    }
                    ratios.sort_by(f64::total_cmp);
                    let ratio = ratios[10];
                    let budget = if quantized && rows >= 2 { 0.95 } else { 1.05 };
                    println!("result quantized={quantized} base={base} kind={name} rows={rows} ratio={ratio:.8} budget={budget:.2}");
                    if ratio >= budget {
                        failures.push((quantized, base, name, rows, ratio, budget));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "完整线性层预算失败：{failures:?}");
}
```

## Verification recovery

整理原始数据时，解析器最初只接受行首 sample，漏掉被测试框架的测试名输出前缀黏住的第一条样本。完整性断言发现两轮第一个形状都只有 20 对，因而阻止生成笔记与 CSV；修正为识别行内 sample 后，两轮各 96 形状、每形状对号 0..20 完整。原始计时日志没有改动或重跑。

首次整理命令未设遇错停止，生成笔记失败后继续对尚未恢复的原型运行了普通全测和 Clippy；普通全测通过，Clippy 因临时计时测试的常量 release 断言失败。这些日志明确标为恢复前，不能算恢复后门禁。完成数据校验后再逐字恢复 lib.rs 并删除临时测试，以遇错即停的命令重跑恢复后检查；本次 PR 仅保留负结果笔记和合成 CSV。

## Restored verification

恢复后的普通 `cargo test --manifest-path reference/Cargo.toml --locked`、`cargo clippy --manifest-path reference/Cargo.toml --locked --all-targets -- -D warnings`、`cargo fmt --manifest-path reference/Cargo.toml --check` 与 `git diff --check` 均退出 0，普通单测 40 通过、7 忽略，模型文件测试 15 通过，doc tests 0 项。使用主项目现有的笔记树、格式和归档校验脚本，在本仓库根目录执行，20 篇笔记全部通过，归档与封印均为 0。上游没有 `pnpm run verify-notes` 或主项目 quick 脚本，未复制工具链或声称运行不存在的入口。生产 lib.rs 与固定基线逐字相等；主项目四处 pin 均未改动。

独立审查逐项核对 4032 对 CSV 与原始日志、192 个完整 21 对分组、所有表格统计和失败数，确认一致；三个内嵌 patch 均可应用固定基线、完整测试快照一致，生产恢复与验证范围准确，无阻断项。
