"""Render measured points only from the accepted descriptive projection.

This analysis reads no original ZIP, native program, or validator. Its input
records retain immutable original-return hashes and exact integer timings.
"""
import hashlib
import json
from pathlib import Path

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib.ticker import FixedLocator, FixedFormatter, LogLocator

ROOT = Path(__file__).resolve().parent.parent
OUT = Path(__file__).resolve().parent
SOURCE = ROOT / 'round47-new45-descriptive-analysis/accepted-45-descriptive-pending-aggregate-draft.json'
raw = SOURCE.read_bytes()
data = json.loads(raw)
assert data['accepted_cells'] == 45 and data['accepted_records'] == 85
rows = []
for observation in data['observations']:
    if observation['role'] == 'setup':
        continue
    value = observation['original']
    counts = observation['counts_over_original_reports']
    rows.append({
        'slot': observation['slot'],
        'role': observation['role'],
        'n': observation['n'],
        'environment_ref': observation['environment_ref'],
        'engine_ms': value['engine_ms'],
        'full_control_ms': value['full_control_ms'],
        'builds': value['builds'],
        'resumed_builds': counts['resumed_true_report_count'],
        'parsed_count_sum': counts['files_parsed_sum'],
        'scanned_count_sum': counts['files_scanned_sum'],
        'selected_dependents_count_sum': counts['selected_dependents_sum'],
        'final_complete': counts['last_freshness']['complete'],
        'original_result_pointer': observation['original_result_pointer'],
    })
assert len(rows) == 45 and len({tuple(row['slot']) for row in rows}) == 45
assert all(row['n'] == 1 and row['final_complete'] for row in rows)
projection = {
    'schema': 'complete45-original-observed-plot-points-v1',
    'source': data['source'], 'run_id': data['run_id'], 'attempt': data['attempt'],
    'input_projection': str(SOURCE),
    'input_projection_sha256': hashlib.sha256(raw).hexdigest(),
    'scope': '45 observed points, N=1 per cell; no interpolation, fitting, confidence intervals or statistical/causal performance gate.',
    'timing': 'The scale figure divides engine_ms and full_control_ms by 1000 to plot seconds; the fanout figure retains original milliseconds. engine_ms sums actual closure builds. Full control is separate. Neither includes parity/setup.',
    'environments': data['environments'], 'rows': rows,
}
(OUT / 'original-plot-points.json').write_text(json.dumps(projection, ensure_ascii=False, indent=2) + '\n')

plt.rcParams.update({'font.family': 'DejaVu Sans', 'font.size': 10, 'axes.spines.top': False, 'axes.spines.right': False, 'axes.titleweight': 'bold', 'savefig.facecolor': '#ffffff'})
profiles = ['no_op', 'body', 'api', 'config', 'batch_1', 'batch_10', 'batch_100', 'batch_1000']
scales = [1000, 5000, 10000, 50000, 100000]
fig, axes = plt.subplots(2, 4, figsize=(14.8, 8.4), sharex=True, sharey=True)
for ax, profile in zip(axes.flat, profiles):
    selected = sorted([row for row in rows if row['slot'][0] == profile], key=lambda row: row['slot'][1])
    assert [row['slot'][1] for row in selected] == scales
    ax.scatter(scales, [row['engine_ms'] / 1000 for row in selected], s=42, marker='o', c='#1967b3', label='Incremental closure', zorder=3)
    ax.scatter(scales, [row['full_control_ms'] / 1000 for row in selected], s=44, marker='x', c='#b75a23', linewidths=1.7, label='Full control', zorder=3)
    ax.set_xscale('log'); ax.set_yscale('log'); ax.set_xlim(800, 130000); ax.set_ylim(0.008, 6000)
    ax.xaxis.set_major_locator(FixedLocator(scales)); ax.xaxis.set_major_formatter(FixedFormatter(['1k', '5k', '10k', '50k', '100k']))
    ax.set_title(profile, fontsize=12, pad=23)
    ax.text(0.5, 1.025, 'Builds: ' + ' / '.join(str(row['builds']) for row in selected), transform=ax.transAxes, ha='center', fontsize=8.8, color='#465563')
    ax.grid(axis='y', alpha=0.20); ax.tick_params(axis='x', labelsize=9)
handles, labels = axes[0, 0].get_legend_handles_labels()
fig.legend(handles, labels, loc='upper center', ncol=2, frameon=False, bbox_to_anchor=(0.5, 0.935))
fig.suptitle('Incremental scale observations: every registered profile and size', fontsize=18, x=0.5, y=0.988)
fig.supxlabel('Fixture files (log scale)', y=0.052, fontsize=11)
fig.supylabel('Reported engine time (seconds, log scale)', x=0.009, fontsize=11)
fig.text(0.5, 0.017, 'N=1 per cell; independent hosted VMs. Points are original measurements, without fitted curves or confidence intervals.\nSource f97c5068 | run 38026411200 / attempt 1 | setup and parity timing remain separate in the full report.', ha='center', fontsize=8.7, color='#465563')
fig.subplots_adjust(left=0.063, right=0.989, top=0.845, bottom=0.13, hspace=0.40, wspace=0.15)
for suffix in ('png', 'svg'):
    fig.savefig(OUT / f'incremental-scale-observations.{suffix}', dpi=180)
plt.close(fig)

fanout = sorted([row for row in rows if row['role'] == 'fanout'], key=lambda row: row['slot'][1])
assert [row['slot'][1] for row in fanout] == [1, 4, 16, 64, 128]
fig, axes = plt.subplots(1, 2, figsize=(11.8, 5.2))
x = list(range(5)); labels = [str(row['slot'][1]) for row in fanout]
axes[0].scatter(x, [row['engine_ms'] for row in fanout], s=64, c='#1967b3', label='Incremental closure', zorder=3)
axes[0].scatter(x, [row['full_control_ms'] for row in fanout], s=68, marker='x', linewidths=2, c='#b75a23', label='Full control', zorder=3)
axes[0].set_ylabel('Reported engine time (ms)'); axes[0].set_ylim(0, 270)
axes[0].set_title('Observed elapsed time'); axes[0].legend(frameon=False, fontsize=9)
first = [row['builds'] - row['resumed_builds'] for row in fanout]
assert first == [1] * 5
axes[1].bar(x, first, color='#1967b3', width=0.55, label='First build')
axes[1].bar(x, [row['resumed_builds'] for row in fanout], bottom=first, color='#9ebde0', width=0.55, label='Resumed builds')
for i, row in enumerate(fanout):
    axes[1].text(i, row['builds'] + 0.3, str(row['builds']), ha='center', fontsize=10)
axes[1].set_ylim(0, 19); axes[1].set_yticks([0, 4, 8, 12, 16]); axes[1].set_ylabel('Actual build count')
axes[1].set_title('All closures reached ready'); axes[1].legend(frameon=False, fontsize=9, loc='upper left')
for ax in axes:
    ax.set_xticks(x, labels); ax.set_xlabel('Registered fanout'); ax.grid(axis='y', alpha=0.2); ax.set_axisbelow(True)
fig.suptitle('Bounded fanout closure: five original observations', fontsize=17, y=0.98)
fig.text(0.5, 0.025, 'N=1 per cell. Actual fixture files: 2, 5, 17, 65, 129. Dirty budget: 8.\nTiming excludes fixture setup and parity; the separately reported whole-fixture interval includes them.', ha='center', fontsize=9, color='#465563')
fig.subplots_adjust(left=0.075, right=0.98, top=0.81, bottom=0.20, wspace=0.28)
for suffix in ('png', 'svg'):
    fig.savefig(OUT / f'fanout-closure-observations.{suffix}', dpi=180)
plt.close(fig)
print(json.dumps({'status': 'rendered', 'original_point_count': 45, 'charts': 2, 'formats': ['png', 'svg'], 'measurement_calls': 0, 'validator_calls': 0, 'native_calls': 0}))
