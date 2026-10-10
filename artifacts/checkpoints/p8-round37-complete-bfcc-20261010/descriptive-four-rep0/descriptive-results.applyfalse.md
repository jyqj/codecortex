# P8-006 描述性结果候选：四个预登记 rep0

**apply_now=false；4/5 规模，36 个主阶段 + 5 个 fanout = 41 个已接受观察。100k pending。**

原执行 source `9cc6bf49f6dd81e4069a8004eed49addba0ab79b`；run `38013753078` / attempt 1；本 run 独立 build 的 ELF SHA256 `c452d4145c9ccb072f3e9e3f68399b15e5b02274aade16eeec69875f28d29610`。原计划仍 N=30、150 片、1500 观察。这里每个已观察 cell 的描述性 N=1，未改变注册。

每个 slot 独立展示环境。相同 hostname 不能证明相同宿主；5k 的 CPU 型号与其余三片不同。没有跨 host 合并、回归拟合、尾分位、速度倍数或统计性能通过声明。

下列所有时间均为原整数，单位见列名。`—` 表示原字段没有提供，不是 0。engine、index wall、outer wall 存在嵌套，不能相加。prepare_snapshot/full_staging 是 prepare 子项，不能再加到 total。完整 JSON 保留全部41个原 measurement 对象。

## 1000 文件 / rep0 / 原 host `runnervmmprz5`

CPU `AMD EPYC 7763 64-Core Processor`；分配 CPU `4`；kernel `6.17.0-1022-azure`；环境 ID `run-38013753078/scale-1000/rep-0`。

| 阶段 | engine ms | index wall µs | outer A µs | full B ms | parity µs | A builds |
| --- | --- | --- | --- | --- | --- | --- |
| cold | 1805 | 1807207 | — | 1807 | 1613787 | 1 |
| no_op | 17 | 18070 | 18248 | 1754 | 1455113 | 1 |
| body | 1488 | 1490530 | 1490810 | 1781 | 1483138 | 1 |
| api | 731 | 733568 | 733912 | 1748 | 1497232 | 1 |
| config | 1065 | 1067537 | 1067828 | 1712 | 1476069 | 1 |
| batch_1 | 21 | 23610 | 23789 | 2053 | 1476594 | 1 |
| batch_10 | 105 | 107452 | 107722 | 1654 | 1494953 | 1 |
| batch_100 | 356 | 358585 | 358875 | 1678 | 1479219 | 1 |
| batch_1000 | 2238 | 2241362 | 2241635 | 1760 | 1550985 | 1 |

A 的原 build_timing（µs；包含 resume 时保持原 driver 汇总）：

| 阶段 | prepare_us | commit_write_us | postprocess_compute_us | postprocess_apply_us | between_stages_us | total_us | prepare_snapshot_us | full_staging_us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 1639831 | 85535 | 28858 | 21201 | 30151 | 1805578 | 23978 | 1168550 |
| no_op | 13492 | 371 | 2958 | 89 | 145 | 17058 | 0 | — |
| body | 480209 | 939117 | 24279 | 19852 | 25481 | 1488940 | 1117 | — |
| api | 212033 | 469164 | 20749 | 20857 | 9305 | 732111 | 1296 | — |
| config | 321211 | 686183 | 23419 | 15523 | 19027 | 1065367 | 329 | — |
| batch_1 | 11904 | 6828 | 2820 | 288 | 203 | 22047 | 157 | — |
| batch_10 | 22980 | 78639 | 3141 | 330 | 1096 | 106190 | 575 | — |
| batch_100 | 76752 | 270690 | 3164 | 762 | 5724 | 357094 | 3751 | — |
| batch_1000 | 605541 | 1597432 | 3820 | 645 | 31156 | 2238597 | 24792 | — |

B full control 的原 build_timing（µs，与 A 分开）：

| 阶段 | prepare_us | commit_write_us | postprocess_compute_us | postprocess_apply_us | between_stages_us | total_us | prepare_snapshot_us | full_staging_us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 1662265 | 72663 | 25120 | 20817 | 27121 | 1807987 | 25612 | 1137838 |
| no_op | 1586694 | 87615 | 24120 | 20248 | 35785 | 1754466 | 22966 | 1138368 |
| body | 1610754 | 94742 | 25643 | 20363 | 30508 | 1782014 | 23219 | 1100902 |
| api | 1587501 | 91717 | 21530 | 21732 | 26403 | 1748885 | 28035 | 1080383 |
| config | 1542032 | 99110 | 23618 | 20272 | 27996 | 1713031 | 26165 | 1070630 |
| batch_1 | 1890062 | 88977 | 24306 | 21378 | 28920 | 2053645 | 21635 | 1410064 |
| batch_10 | 1490163 | 92363 | 24113 | 21116 | 26832 | 1654590 | 26890 | 1055371 |
| batch_100 | 1516296 | 87164 | 23415 | 19890 | 31718 | 1678486 | 25495 | 1076749 |
| batch_1000 | 1591055 | 88681 | 22523 | 20424 | 38061 | 1760748 | 24532 | 1120196 |

原 phase_timing（ms；不等于完整 wall 的可加分解）：

| 阶段 | scan_diff_ms | parse_ms | resolve_ms | write_ms | postprocess_ms | analysis_ms |
| --- | --- | --- | --- | --- | --- | --- |
| cold | 38 | 292 | 115 | 85 | 45 | 3 |
| no_op | 13 | 0 | 0 | 0 | 0 | 2 |
| body | 9 | 356 | 112 | 907 | 40 | 3 |
| api | 9 | 150 | 50 | 448 | 36 | 3 |
| config | 10 | 237 | 73 | 662 | 35 | 3 |
| batch_1 | 9 | 1 | 0 | 6 | 0 | 2 |
| batch_10 | 10 | 9 | 1 | 77 | 0 | 3 |
| batch_100 | 13 | 47 | 11 | 266 | 0 | 3 |
| batch_1000 | 39 | 426 | 114 | 1557 | 0 | 3 |

A 原工作计数（跨 resume 求和是工作量，不是去重文件数）：

| 阶段 | files_scanned | files_parsed | files_added | files_updated | files_removed | files_skipped | A complete 序列 | parity |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 1000 | 1000 | 1000 | 0 | 0 | 0 | true | equal |
| no_op | 1000 | 0 | 0 | 0 | 0 | 1000 | true | equal |
| body | 1000 | 1 | 0 | 1 | 0 | 999 | true | equal |
| api | 1000 | 1 | 0 | 1 | 0 | 999 | true | equal |
| config | 1000 | 0 | 0 | 0 | 0 | 1000 | true | equal |
| batch_1 | 1000 | 1 | 0 | 1 | 0 | 999 | true | equal |
| batch_10 | 1000 | 10 | 0 | 10 | 0 | 990 | true | equal |
| batch_100 | 1000 | 100 | 0 | 100 | 0 | 900 | true | equal |
| batch_1000 | 1000 | 1000 | 0 | 1000 | 0 | 0 | true | equal |

冷态实际 DB：files=1000、symbols=5571、chunks=6370；edges 各表为 `{"call_edges": 4974, "semantic_edges": 5990, "test_edges": 0}`（不相加当去重总数）。vectors=`{"count": null, "reason": "semantic explicitly disabled; no provider installed", "state": "disabled"}`。各阶段两侧全部表计数、闭包及 raw 指针在 JSON。

原 ZIP Git `a9c02bf7d6e483ce74b4ca856276faf211bbdbcc`，SHA256 `955fbe772b309790977ba858b396ea373559d87c1b73c3d3e973d2542484ff0d`；原接受 review Git `27aa5faf091ef500c30e6b5d630c58a26f4fb5ba`。

## 5000 文件 / rep0 / 原 host `runnervmmprz5`

CPU `AMD EPYC 9V74 80-Core Processor`；分配 CPU `4`；kernel `6.17.0-1022-azure`；环境 ID `run-38013753078/scale-5000/rep-0`。

| 阶段 | engine ms | index wall µs | outer A µs | full B ms | parity µs | A builds |
| --- | --- | --- | --- | --- | --- | --- |
| cold | 10519 | 10524101 | — | 9503 | 7748960 | 1 |
| no_op | 42 | 45014 | 45331 | 11327 | 7028493 | 1 |
| body | 7765 | 7774334 | 7775059 | 10081 | 7160312 | 2 |
| api | 3631 | 3638780 | 3639086 | 10001 | 7231143 | 1 |
| config | 5677 | 5681642 | 5682002 | 9536 | 7229079 | 1 |
| batch_1 | 95 | 99090 | 99370 | 9592 | 7219240 | 1 |
| batch_10 | 173 | 176592 | 176791 | 9726 | 7254532 | 1 |
| batch_100 | 591 | 594965 | 595196 | 9404 | 7253833 | 1 |
| batch_1000 | 3332 | 3335975 | 3336454 | 9694 | 7188715 | 1 |

A 的原 build_timing（µs；包含 resume 时保持原 driver 汇总）：

| 阶段 | prepare_us | commit_write_us | postprocess_compute_us | postprocess_apply_us | between_stages_us | total_us | prepare_snapshot_us | full_staging_us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 9824924 | 285872 | 83690 | 178804 | 146539 | 10519832 | 73166 | 8029196 |
| no_op | 38300 | 888 | 3348 | 293 | 289 | 43121 | 0 | — |
| body | 1780899 | 5602549 | 93097 | 180653 | 110620 | 7767823 | 5319 | — |
| api | 697247 | 2640265 | 87817 | 167653 | 41267 | 3634253 | 4871 | — |
| config | 1339280 | 4035602 | 89530 | 143703 | 69432 | 5677550 | 1831 | — |
| batch_1 | 83718 | 7951 | 3421 | 1213 | 465 | 96772 | 134 | — |
| batch_10 | 45101 | 124345 | 3389 | 699 | 793 | 174331 | 589 | — |
| batch_100 | 79948 | 505095 | 3457 | 835 | 3357 | 592696 | 1076 | — |
| batch_1000 | 482823 | 2814182 | 3930 | 1019 | 31375 | 3333332 | 10383 | — |

B full control 的原 build_timing（µs，与 A 分开）：

| 阶段 | prepare_us | commit_write_us | postprocess_compute_us | postprocess_apply_us | between_stages_us | total_us | prepare_snapshot_us | full_staging_us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 8818973 | 282833 | 85599 | 177836 | 138455 | 9503698 | 59819 | 7153819 |
| no_op | 10581083 | 332268 | 85478 | 158558 | 170706 | 11328097 | 57830 | 8842574 |
| body | 9327359 | 335929 | 87753 | 161496 | 169864 | 10082404 | 56765 | 7548654 |
| api | 9247904 | 325598 | 92252 | 166258 | 170488 | 10002502 | 58293 | 7519982 |
| config | 8759447 | 341066 | 92591 | 183531 | 160294 | 9536931 | 57198 | 7114524 |
| batch_1 | 8861114 | 265475 | 92291 | 151641 | 222883 | 9593406 | 58121 | 7129051 |
| batch_10 | 9003004 | 275602 | 92106 | 157395 | 198518 | 9726627 | 58794 | 7314333 |
| batch_100 | 8575676 | 266205 | 94822 | 228333 | 239807 | 9404846 | 56800 | 6983931 |
| batch_1000 | 8934167 | 337391 | 91928 | 163906 | 167098 | 9694493 | 66725 | 7137108 |

原 phase_timing（ms；不等于完整 wall 的可加分解）：

| 阶段 | scan_diff_ms | parse_ms | resolve_ms | write_ms | postprocess_ms | analysis_ms |
| --- | --- | --- | --- | --- | --- | --- |
| cold | 138 | 1052 | 531 | 285 | 256 | 5 |
| no_op | 37 | 0 | 0 | 0 | 0 | 3 |
| body | 67 | 1135 | 571 | 5392 | 261 | 8 |
| api | 32 | 481 | 178 | 2589 | 247 | 4 |
| config | 33 | 906 | 397 | 3874 | 227 | 4 |
| batch_1 | 33 | 1 | 48 | 7 | 0 | 3 |
| batch_10 | 34 | 8 | 1 | 123 | 0 | 3 |
| batch_100 | 35 | 34 | 8 | 501 | 0 | 3 |
| batch_1000 | 55 | 321 | 94 | 2778 | 0 | 3 |

A 原工作计数（跨 resume 求和是工作量，不是去重文件数）：

| 阶段 | files_scanned | files_parsed | files_added | files_updated | files_removed | files_skipped | A complete 序列 | parity |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 5000 | 5000 | 5000 | 0 | 0 | 0 | true | equal |
| no_op | 5000 | 0 | 0 | 0 | 0 | 5000 | true | equal |
| body | 10000 | 1 | 0 | 1 | 0 | 9999 | false,true | equal |
| api | 5000 | 1 | 0 | 1 | 0 | 4999 | true | equal |
| config | 5000 | 0 | 0 | 0 | 0 | 5000 | true | equal |
| batch_1 | 5000 | 1 | 0 | 1 | 0 | 4999 | true | equal |
| batch_10 | 5000 | 10 | 0 | 10 | 0 | 4990 | true | equal |
| batch_100 | 5000 | 100 | 0 | 100 | 0 | 4900 | true | equal |
| batch_1000 | 5000 | 1000 | 0 | 1000 | 0 | 4000 | true | equal |

冷态实际 DB：files=5000、symbols=27814、chunks=31813；edges 各表为 `{"call_edges": 24577, "semantic_edges": 29913, "test_edges": 0}`（不相加当去重总数）。vectors=`{"count": null, "reason": "semantic explicitly disabled; no provider installed", "state": "disabled"}`。各阶段两侧全部表计数、闭包及 raw 指针在 JSON。

原 ZIP Git `4221c7571ea3d9bb1fd166e391d40bf6360427fa`，SHA256 `cb94225f6f3a8a5a75d1072540de9bf12c08d2f7b3249a6e78d3bc95d5c3c71f`；原接受 review Git `27aa5faf091ef500c30e6b5d630c58a26f4fb5ba`。

## 10000 文件 / rep0 / 原 host `runnervmmprz5`

CPU `AMD EPYC 7763 64-Core Processor`；分配 CPU `4`；kernel `6.17.0-1022-azure`；环境 ID `run-38013753078/scale-10000/rep-0`。

| 阶段 | engine ms | index wall µs | outer A µs | full B ms | parity µs | A builds |
| --- | --- | --- | --- | --- | --- | --- |
| cold | 20733 | 20742982 | — | 20539 | 18291120 | 1 |
| no_op | 89 | 93456 | 94041 | 21162 | 18261299 | 1 |
| body | 17820 | 17841576 | 17842491 | 20775 | 18706554 | 3 |
| api | 7776 | 7788932 | 7789180 | 21439 | 18701075 | 1 |
| config | 13340 | 13353520 | 13354479 | 20272 | 18553487 | 2 |
| batch_1 | 101 | 108339 | 108600 | 21038 | 18545326 | 1 |
| batch_10 | 209 | 214196 | 214424 | 20217 | 18436366 | 1 |
| batch_100 | 769 | 774115 | 774388 | 20458 | 18443110 | 1 |
| batch_1000 | 3810 | 3816820 | 3829848 | 20486 | 18602812 | 1 |

A 的原 build_timing（µs；包含 resume 时保持原 driver 汇总）：

| 阶段 | prepare_us | commit_write_us | postprocess_compute_us | postprocess_apply_us | between_stages_us | total_us | prepare_snapshot_us | full_staging_us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 19241501 | 583511 | 201413 | 330426 | 377673 | 20734527 | 912312 | 14177490 |
| no_op | 82332 | 1758 | 5181 | 695 | 587 | 90557 | 0 | — |
| body | 4662019 | 12381339 | 213881 | 331776 | 237265 | 17826289 | 14701 | — |
| api | 1725948 | 5430205 | 211223 | 323190 | 90703 | 7781272 | 9211 | — |
| config | 3261229 | 9404423 | 216115 | 297512 | 163003 | 13342287 | 5201 | — |
| batch_1 | 80695 | 14797 | 5209 | 3162 | 919 | 104785 | 140 | — |
| batch_10 | 88785 | 113474 | 5382 | 1602 | 1329 | 210576 | 554 | — |
| batch_100 | 133494 | 626330 | 5261 | 1503 | 3921 | 770512 | 2692 | — |
| batch_1000 | 616644 | 3138171 | 5831 | 2079 | 49472 | 3812200 | 20473 | — |

B full control 的原 build_timing（µs，与 A 分开）：

| 阶段 | prepare_us | commit_write_us | postprocess_compute_us | postprocess_apply_us | between_stages_us | total_us | prepare_snapshot_us | full_staging_us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 19055387 | 706669 | 203575 | 305889 | 269271 | 20540793 | 770198 | 14191643 |
| no_op | 19413002 | 809171 | 201308 | 347424 | 402553 | 21173461 | 772786 | 14347536 |
| body | 19126234 | 692775 | 203674 | 343472 | 410210 | 20776368 | 680249 | 14220617 |
| api | 19754344 | 825185 | 222300 | 304867 | 333911 | 21440611 | 817431 | 14741316 |
| config | 18530355 | 824869 | 206993 | 331456 | 380082 | 20273757 | 265874 | 14198334 |
| batch_1 | 19341069 | 813051 | 209721 | 315317 | 360004 | 21039165 | 675593 | 14718239 |
| batch_10 | 18530667 | 802181 | 211370 | 344510 | 329941 | 20218672 | 250472 | 14029085 |
| batch_100 | 18769380 | 694424 | 210344 | 353083 | 432432 | 20459666 | 699409 | 13957455 |
| batch_1000 | 18807732 | 853057 | 222499 | 342853 | 261839 | 20487982 | 791735 | 14073971 |

原 phase_timing（ms；不等于完整 wall 的可加分解）：

| 阶段 | scan_diff_ms | parse_ms | resolve_ms | write_ms | postprocess_ms | analysis_ms |
| --- | --- | --- | --- | --- | --- | --- |
| cold | 336 | 2603 | 1210 | 583 | 521 | 8 |
| no_op | 81 | 0 | 0 | 1 | 0 | 5 |
| body | 207 | 2972 | 1462 | 11929 | 520 | 16 |
| api | 74 | 1261 | 380 | 5324 | 520 | 6 |
| config | 135 | 2159 | 960 | 9024 | 496 | 10 |
| batch_1 | 77 | 1 | 1 | 14 | 0 | 5 |
| batch_10 | 76 | 8 | 3 | 111 | 0 | 5 |
| batch_100 | 75 | 43 | 12 | 621 | 0 | 5 |
| batch_1000 | 105 | 382 | 107 | 3102 | 0 | 5 |

A 原工作计数（跨 resume 求和是工作量，不是去重文件数）：

| 阶段 | files_scanned | files_parsed | files_added | files_updated | files_removed | files_skipped | A complete 序列 | parity |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 10000 | 10000 | 10000 | 0 | 0 | 0 | true | equal |
| no_op | 10000 | 0 | 0 | 0 | 0 | 10000 | true | equal |
| body | 30000 | 1 | 0 | 1 | 0 | 29999 | false,false,true | equal |
| api | 10000 | 1 | 0 | 1 | 0 | 9999 | true | equal |
| config | 20000 | 0 | 0 | 0 | 0 | 20000 | false,true | equal |
| batch_1 | 10000 | 1 | 0 | 1 | 0 | 9999 | true | equal |
| batch_10 | 10000 | 10 | 0 | 10 | 0 | 9990 | true | equal |
| batch_100 | 10000 | 100 | 0 | 100 | 0 | 9900 | true | equal |
| batch_1000 | 10000 | 1000 | 0 | 1000 | 0 | 9000 | true | equal |

冷态实际 DB：files=10000、symbols=55620、chunks=63619；edges 各表为 `{"call_edges": 49083, "semantic_edges": 59819, "test_edges": 0}`（不相加当去重总数）。vectors=`{"count": null, "reason": "semantic explicitly disabled; no provider installed", "state": "disabled"}`。各阶段两侧全部表计数、闭包及 raw 指针在 JSON。

原 ZIP Git `d9961e393040ee553eadc85ff6bed7a5b1b79492`，SHA256 `41919dd2253f2a3cad5b75d7abc7be89aec10b2667dcceba308b1e5ea8238bf2`；原接受 review Git `613a418a178a9a11d63247088ed4b91a9e25e4fd`。

## 50000 文件 / rep0 / 原 host `runnervmmprz5`

CPU `AMD EPYC 7763 64-Core Processor`；分配 CPU `4`；kernel `6.17.0-1022-azure`；环境 ID `run-38013753078/scale-50000/rep-0`。

| 阶段 | engine ms | index wall µs | outer A µs | full B ms | parity µs | A builds |
| --- | --- | --- | --- | --- | --- | --- |
| cold | 124423 | 124474865 | — | 120004 | 171635550 | 1 |
| no_op | 607 | 622267 | 622657 | 169639 | 177134151 | 1 |
| body | 126588 | 126848491 | 126860872 | 181036 | 183735293 | 12 |
| api | 69717 | 69957406 | 69959212 | 169598 | 147975091 | 5 |
| config | 111265 | 111490842 | 111493912 | 191381 | 189533632 | 9 |
| batch_1 | 654 | 679169 | 680948 | 193522 | 235150953 | 1 |
| batch_10 | 1017 | 1041919 | 1042435 | 198268 | 201513761 | 1 |
| batch_100 | 2158 | 2185701 | 2187493 | 173775 | 178127848 | 1 |
| batch_1000 | 8154 | 8183526 | 8186881 | 201249 | 251693683 | 1 |

A 的原 build_timing（µs；包含 resume 时保持原 driver 汇总）：

| 阶段 | prepare_us | commit_write_us | postprocess_compute_us | postprocess_apply_us | between_stages_us | total_us | prepare_snapshot_us | full_staging_us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 114949399 | 3829924 | 1345328 | 2173173 | 2130662 | 124428488 | 822608 | 91953593 |
| no_op | 571260 | 12462 | 17768 | 4175 | 5638 | 611306 | 0 | — |
| body | 29854470 | 91479007 | 1739560 | 2216143 | 1375900 | 126665120 | 41178 | — |
| api | 10162046 | 41779120 | 7937205 | 9307882 | 576257 | 69762527 | 17151 | — |
| config | 33404276 | 72317284 | 2747240 | 1887773 | 967430 | 111324028 | 28300 | — |
| batch_1 | 600162 | 33898 | 16624 | 11144 | 4137 | 665968 | 100 | — |
| batch_10 | 815789 | 178611 | 17626 | 8866 | 5112 | 1026005 | 754 | — |
| batch_100 | 712151 | 1344495 | 18331 | 9527 | 83739 | 2168245 | 2893 | — |
| batch_1000 | 1347671 | 6552390 | 147307 | 10146 | 107423 | 8164942 | 27929 | — |

B full control 的原 build_timing（µs，与 A 分开）：

| 阶段 | prepare_us | commit_write_us | postprocess_compute_us | postprocess_apply_us | between_stages_us | total_us | prepare_snapshot_us | full_staging_us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 111050827 | 3749214 | 1500169 | 2112935 | 1596201 | 120009350 | 852550 | 88300910 |
| no_op | 158260151 | 5488617 | 1513048 | 2721685 | 1660665 | 169644168 | 847889 | 131943824 |
| body | 165508182 | 9294557 | 1781884 | 3028725 | 1429573 | 181042925 | 854433 | 138494759 |
| api | 145418367 | 4397481 | 3232225 | 2326687 | 14355715 | 169730479 | 842220 | 117453722 |
| config | 174385766 | 9148775 | 2268063 | 3318669 | 2265877 | 191387152 | 842314 | 141354992 |
| batch_1 | 175558523 | 10586307 | 1770600 | 3424909 | 2236021 | 193576364 | 891462 | 146187678 |
| batch_10 | 178676768 | 11418594 | 2147256 | 3579749 | 2452002 | 198274373 | 856661 | 150746186 |
| batch_100 | 162238499 | 4007030 | 1975068 | 2603119 | 2957663 | 173781382 | 840519 | 131204832 |
| batch_1000 | 182865491 | 10646035 | 2143016 | 3511161 | 2158614 | 201324319 | 930177 | 153939084 |

原 phase_timing（ms；不等于完整 wall 的可加分解）：

| 阶段 | scan_diff_ms | parse_ms | resolve_ms | write_ms | postprocess_ms | analysis_ms |
| --- | --- | --- | --- | --- | --- | --- |
| cold | 1726 | 13546 | 6895 | 3822 | 3477 | 35 |
| no_op | 567 | 0 | 3 | 12 | 0 | 17 |
| body | 4451 | 16597 | 8746 | 88511 | 3633 | 232 |
| api | 2029 | 5230 | 2878 | 40378 | 17022 | 167 |
| config | 4170 | 11916 | 17272 | 68049 | 4387 | 181 |
| batch_1 | 589 | 3 | 7 | 32 | 0 | 16 |
| batch_10 | 790 | 12 | 12 | 172 | 0 | 17 |
| batch_100 | 628 | 58 | 21 | 1334 | 0 | 18 |
| batch_1000 | 664 | 517 | 137 | 6504 | 0 | 146 |

A 原工作计数（跨 resume 求和是工作量，不是去重文件数）：

| 阶段 | files_scanned | files_parsed | files_added | files_updated | files_removed | files_skipped | A complete 序列 | parity |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 50000 | 50000 | 50000 | 0 | 0 | 0 | true | equal |
| no_op | 50000 | 0 | 0 | 0 | 0 | 50000 | true | equal |
| body | 600000 | 1 | 0 | 1 | 0 | 599999 | false,false,false,false,false,false,false,false,false,false,false,true | equal |
| api | 250000 | 1 | 0 | 1 | 0 | 249999 | false,false,false,false,true | equal |
| config | 450000 | 0 | 0 | 0 | 0 | 450000 | false,false,false,false,false,false,false,false,true | equal |
| batch_1 | 50000 | 1 | 0 | 1 | 0 | 49999 | true | equal |
| batch_10 | 50000 | 10 | 0 | 10 | 0 | 49990 | true | equal |
| batch_100 | 50000 | 100 | 0 | 100 | 0 | 49900 | true | equal |
| batch_1000 | 50000 | 1000 | 0 | 1000 | 0 | 49000 | true | equal |

冷态实际 DB：files=50000、symbols=278077、chunks=318076；edges 各表为 `{"call_edges": 245140, "semantic_edges": 299076, "test_edges": 0}`（不相加当去重总数）。vectors=`{"count": null, "reason": "semantic explicitly disabled; no provider installed", "state": "disabled"}`。各阶段两侧全部表计数、闭包及 raw 指针在 JSON。

原 ZIP Git `f14e222acb3ba718b30c6bd3fd3919ab252bb476`，SHA256 `575d13e2303aae7755ceec6f3f15a1a04760216baa4de73a5db8bdc47121d376`；原接受 review Git `f24820113d77c649c778f68943a8c36a20531588`。

## 独立 fanout：与主规模时间分栏

均来自1k/rep0承载的独立fixture，实际文件数 N+1，原 dirty/resume=8/128。whole fixture wall 包含两侧建库与 oracle；不能当成主阶段 index wall。

| fanout | fixture files | engine ms | whole fixture µs | builds | first incomplete | 各build complete | final status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 2 | 8 | 272035 | 1 | false | true | compared |
| 4 | 5 | 11 | 280078 | 1 | false | true | compared |
| 16 | 17 | 24 | 318593 | 2 | true | false,true | compared |
| 64 | 65 | 78 | 476287 | 8 | true | false,false,false,false,false,false,false,true | compared |
| 128 | 129 | 172 | 710838 | 16 | true | false,false,false,false,false,false,false,false,false,false,false,false,false,false,false,true | compared |

Fanout 原 build_timing（µs）：

| fanout | prepare_us | commit_write_us | postprocess_compute_us | postprocess_apply_us | between_stages_us | total_us | prepare_snapshot_us | full_staging_us |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fanout-1 | 3200 | 2689 | 2670 | 391 | 13 | 8966 | 2 | — |
| fanout-4 | 4127 | 3924 | 3050 | 361 | 28 | 11494 | 2 | — |
| fanout-16 | 11751 | 6638 | 5772 | 699 | 75 | 24942 | 3 | — |
| fanout-64 | 38214 | 19454 | 21917 | 3596 | 268 | 83474 | 11 | — |
| fanout-128 | 78542 | 43923 | 45507 | 9461 | 584 | 178064 | 17 | — |

Fanout 原 phase_timing（ms）：

| fanout | scan_diff_ms | parse_ms | resolve_ms | write_ms | postprocess_ms | analysis_ms |
| --- | --- | --- | --- | --- | --- | --- |
| fanout-1 | 2 | 0 | 0 | 2 | 0 | 2 |
| fanout-4 | 2 | 1 | 0 | 3 | 0 | 2 |
| fanout-16 | 5 | 4 | 0 | 5 | 0 | 4 |
| fanout-64 | 10 | 16 | 0 | 13 | 0 | 16 |
| fanout-128 | 20 | 32 | 0 | 36 | 0 | 32 |

## 100k 与整体状态

100k/rep0：pending；measurement/environment/terminal 均 null。不以旧 G8/GW4 或独立 diagnostic 前缀填补。当前描述性五规模交付尚缺一个端点，006 不在本候选中关闭；原完整150研究与G8 release均未获得通过。

固定主线 b9 与原 G9cc6 全 tree 相等的桥单列保留；原执行 identity 仍为 G9cc6。未来只可在接受同 cohort 的真实100k原件后追加其观察，不能覆盖本4片时态。

完整精确数据：`a7593a266475e99d6a4c14f652b1ac27f62eb1c707e25fef9c90e80c320415f1` / `descriptive-results.applyfalse.json`。派生仅读取既有原输出与已解包raw，未调用任何原validator、ELF、build、CRC、下载或测量。
