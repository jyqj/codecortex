#!/usr/bin/env python3
"""Native PID resource evidence hard gate; null/missing never becomes zero.
Only the owner-calibrated platform's explicit rawclock/timebase->ns is eligible.
"""
import argparse,json,pathlib

def uint(value):
    return type(value) is int and 0<=value<=(1<<64)-1

def check(data):
    issues=[];groups={role:[] for role in ['server','runner']};samples=data.get('samples',[])
    previous_finish=None
    for role in groups:
        if not uint(data.get(role+'_pid')) or data.get(role+'_pid')==0:
            issues.append({'role':role,'reason':'invalid_owned_pid'})
    for n,row in enumerate(samples):
        if not(uint(row.get('started_us')) and uint(row.get('finished_us')) and row['finished_us']>=row['started_us']):
            issues.append({'sample':n,'reason':'missing_or_invalid_interval'})
        else:
            if previous_finish is not None and row['started_us']<previous_finish:
                issues.append({'sample':n,'reason':'unordered_or_overlapping_observer_intervals'})
            previous_finish=row['finished_us']
        for role in groups:
            snapshot=row.get(role)
            if not isinstance(snapshot,dict):issues.append({'sample':n,'role':role,'reason':'native_snapshot_unavailable'});continue
            if snapshot.get('pid')!=data.get(role+'_pid'):issues.append({'sample':n,'role':role,'reason':'wrong_pid'});continue
            required=['resident_bytes','threads','cpu_user_raw','cpu_system_raw','cpu_user_ns','cpu_system_ns','timebase_numer','timebase_denom']
            if any(not uint(snapshot.get(k)) for k in required):issues.append({'sample':n,'role':role,'reason':'missing_native_dimension'});continue
            if snapshot.get('resident_method')!='libproc proc_taskinfo resident-size bytes; PID only; not a process tree':
                issues.append({'sample':n,'role':role,'reason':'unrecognized_resident_method'})
            if snapshot['threads']==0:
                issues.append({'sample':n,'role':role,'reason':'zero_threads_for_live_process'})
            if not snapshot['timebase_numer'] or not snapshot['timebase_denom'] or snapshot.get('cpu_time_unit')!='mach_ticks_calibrated_Darwin25.6_aarch64' or snapshot.get('kernel_release')!='25.6.0' or (snapshot['timebase_numer'],snapshot['timebase_denom'])!=(125,3):issues.append({'sample':n,'role':role,'reason':'uncalibrated_or_invalid_raw_clock'});continue
            for field in ['cpu_user','cpu_system']:
                expected=snapshot[field+'_raw']*snapshot['timebase_numer']//snapshot['timebase_denom']
                if expected>(1<<64)-1 or expected!=snapshot[field+'_ns']:issues.append({'sample':n,'role':role,'reason':'raw_to_ns_mismatch','field':field})
            groups[role].append(snapshot)
    report={}
    for role,rows in groups.items():
        if len(rows)<2:issues.append({'role':role,'reason':'fewer_than_two_native_samples','available':len(rows)})
        if rows:
            reference=rows[0]
            for n,row in enumerate(rows[1:],1):
                for key in ['pid','kernel_release','cpu_time_unit','timebase_numer','timebase_denom','resident_method']:
                    if row[key]!=reference[key]:issues.append({'role':role,'sample':n,'reason':'identity_clock_inconsistent','key':key})
                for field in ['cpu_user_raw','cpu_system_raw','cpu_user_ns','cpu_system_ns']:
                    if row[field]<rows[n-1][field]:issues.append({'role':role,'sample':n,'reason':'nonmonotonic_cpu','field':field})
            delta={field:rows[-1][field]-reference[field] for field in ['cpu_user_ns','cpu_system_ns']}
        else:delta=None
        if any(issue.get('role')==role for issue in issues):delta=None
        report[role]={'valid_native_samples':len(rows),'observed_max_resident_bytes':max((r['resident_bytes'] for r in rows),default=None),'observed_max_threads':max((r['threads'] for r in rows),default=None),'checked_cpu_delta':delta}
    return {'status':'passed_native_resource_proof' if not issues else 'blocked_native_resource_proof','exit_code':0 if not issues else 1,'sample_intervals':samples,'issues':issues,'resources':report,'limits':['actual owned server+runner PIDs only;not process-tree','observed sampled max not absolutepeak','everymissing interval retained;uncalibrated clocks not inferred zero','doesnot prove query/source/performance gates']}

def main():
    p=argparse.ArgumentParser();p.add_argument('--samples',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args();assert not a.output.exists();r=check(json.loads(a.samples.read_text()));a.output.write_text(json.dumps(r,indent=2)+'\n');raise SystemExit(r['exit_code'])
if __name__=='__main__':main()
