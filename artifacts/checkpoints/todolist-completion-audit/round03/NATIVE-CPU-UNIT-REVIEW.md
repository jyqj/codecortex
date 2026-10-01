# 原生资源单位独立校准

当前观察环境：macOS 26.6.2 / Darwin 25.6.0 / ARM64_T6031，内核标识 `xnu-12377.161.14~5`；SDK MacOSX15.4。本机 API ABI 独立 C 检验为 TaskInfo 96 bytes、threadnum offset84、flavor4，线程数1→新增6线程后7→join后1。

## CPU 单位实测

独立 `native-taskinfo-cpu-calibration-v2.c` 进行了5轮约300ms忙算，分别采 `PROC_PIDTASKINFO` 与 `getrusage(RUSAGE_SELF)` 计数差分，并读取 `mach_timebase_info`。全部原始观察保存 `native-taskinfo-cpu-observation-v2.json`，actual exit0。

- timebase：numer125 / denom3。
- `proc raw CPU delta / rusage CPU ns delta`：约0.02399926～0.02399967。
- `proc raw × numer / denom / rusage CPU ns delta`：约0.99996919～0.99998638。
- 因而本机 `pti_total_user/system` 是 Mach absolute-time ticks，不能直接写为ns。误标ns将少报约41.67倍。
- 采用 checked `u128(raw) * numer / denom` 转ns，并保留raw/timebase、OS/kernel/SDK、来源/归属和校准说明。未知平台不能沿用未经验证的单位。

初次实验代码采用了待检验的“raw=ns”断言，真实退出4；partial JSON及 `native-taskinfo-cpu-first-attempt.json` 保留，不把初次失败写为通过。第二版中立验证两种候选解释并完整运行，非覆盖旧观察。

## 官方源码与证据等级

Apple公开XNU的[fill_taskprocinfo实现](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/bsd_kern.c#L1016)把recount累计Mach时间赋给TaskInfo计数，与实测一致；main分支不当成当前运行内核的精确源码证明。当前单位结论来自本机实测，不仅来自字段命名或main源码推断。

该实现从phys_mem ledger取得`pti_resident_size`；报告应称“libproc proc_taskinfo resident-size bytes”，注明API方法，不未经核对等同于Mach resident pages、physical footprint或进程树总RSS。抽样最大值也不叫绝对瞬时峰值。进程归属/CPU累计与per-query成本分开，不把runner当server。
