# Agent Note: 同一次前向复用 attention 输出与分数缓冲

Status: proposed

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

## Acceptance criteria

暖缓冲地址测试先红后绿；多头、边界宽度、空行、非零 offset、缩小形状与污染缓冲逐位一致；固定真实模型 KV、隐藏状态和分布一致；Rust 1.75、测试、Clippy、fmt 通过。完整六层 attention 配对测量包含工作区构造与析构，量化 192 宽单行中位比小于 0.99，其余形状小于 1.03；全部形状一次判断。附完整模型 bench 表，不把局部收益当成端到端预算达标。

## Risks

output 和 scores 容量跨后续投影及前馈存活，持有期变长，不能宣称整次前向峰值内存下降。输出必须清零后再累加，缩小形状必须截断；scores 不可见尾部不能参与 softmax。
