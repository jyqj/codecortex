"""Parser/falsification controls only; real kernel controls run separately in CI."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p7_offline_trace as trace


def exec_line(command):
    return 'execve(' + json.dumps(command[0]) + ', ' + json.dumps(command) + ', 0x1 /* 4 vars */) = 0'


def terminal(code=0):
    return f'exit_group({code}) = ?\n+++ exited with {code} +++\n'


class TraceVerifierTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.prefix = self.root / 'syscalls'
        self.command = ['/fixture/product', 'mcp', '--project-path', '/fixture/project']

    def write(self, pid, text):
        (self.root / f'syscalls.{pid}').write_text(text)

    def inspect(self):
        return trace.inspect_tree(trace.read_traces(self.prefix), 10, self.command)

    def test_clean_fork_thread_and_failed_clone_are_all_accounted(self):
        self.write(10, exec_line(self.command) + '\nclone3({}, 88) = -1 ENOSYS (Function not implemented)\n'
                   'clone(child_stack=0x1, flags=CLONE_VM|CLONE_THREAD) = 11\nfork() = 12\n' + terminal())
        self.write(11, 'exit(0) = ?\n+++ exited with 0 +++\n')
        self.write(12, exec_line(['/usr/bin/git', 'status']) + '\n' + terminal(128))
        report = self.inspect()
        self.assertEqual(set(report['processes_and_threads']), {10, 11, 12})
        self.assertEqual(report['attempts'], [])

    def test_child_socket_sigsys_is_rejected_even_when_parent_exits_zero(self):
        self.write(10, exec_line(self.command) + '\nfork() = 11\nwait4(11, [], 0, NULL) = 11\n' + terminal())
        self.write(11, 'socket(AF_INET, SOCK_STREAM, IPPROTO_IP) = ?\n+++ killed by SIGSYS +++\n')
        report = self.inspect()
        self.assertEqual(report['root_terminal'], {'exit_code': 0})
        self.assertTrue(any(item.get('syscall') == 'socket' and item['pid'] == 11
                            for item in report['attempts']))

    def test_grandchild_io_uring_and_swallowed_eperm_are_rejected(self):
        self.write(10, exec_line(self.command) + '\nfork() = 11\n' + terminal())
        self.write(11, 'vfork() = 12\n' + terminal())
        self.write(12, 'io_uring_setup(8, {}) = -1 EPERM (Operation not permitted)\n'
                   'connect(3, {}, 16) = -1 EPERM (Operation not permitted)\n' + terminal())
        report = self.inspect()
        self.assertEqual({item['syscall'] for item in report['attempts']}, {'io_uring_setup', 'connect'})

    def test_unfinished_vfork_resume_preserves_parent_child_edge(self):
        self.write(10, exec_line(self.command) + '\nvfork( <unfinished ...>\n'
                   '--- SIGCHLD {si_signo=SIGCHLD, si_pid=11} ---\n'
                   '<... vfork resumed>) = 11\n' + terminal())
        self.write(11, exec_line(['/usr/bin/git', 'status']) + '\n' + terminal())
        self.assertEqual(set(self.inspect()['processes_and_threads']), {10, 11})

    def test_wrapper_pre_exec_calls_are_separate_but_every_launch_descendant_is_included(self):
        self.write(10, exec_line(['/usr/bin/python3', '/guard.py'])
                   + '\nfork() = 11\nsocket(AF_INET, SOCK_STREAM, 0) = -1 EPERM (Operation not permitted)\n'
                   + exec_line(self.command) + '\n' + terminal())
        self.write(11, exec_line(['/sbin/ldconfig', '-p']) + '\n' + terminal())
        report = self.inspect()
        self.assertEqual(set(report['processes_and_threads']), {10, 11})
        self.assertEqual(report['attempts'], [])

    def test_missing_descendant_missing_exit_extra_root_and_pid_reuse_fail_closed(self):
        for broken, expression in (
                (exec_line(self.command) + '\nfork() = 11\n' + terminal(), 'missing descendant'),
                (exec_line(self.command) + '\n', 'missing complete exit'),
                (exec_line(self.command) + '\n' + terminal() + terminal(), 'PID reuse')):
            with self.subTest(expression=expression):
                self.write(10, broken)
                with self.assertRaisesRegex(ValueError, expression):
                    self.inspect()
        self.write(10, exec_line(self.command) + '\n' + terminal())
        self.write(12, terminal())
        with self.assertRaisesRegex(ValueError, 'extra roots'):
            self.inspect()

    def test_wrong_or_failed_exec_cannot_bind_a_product(self):
        for raw in (exec_line(['/wrong/product', *self.command[1:]]),
                    exec_line(self.command).replace('= 0', '= -1 ENOENT (No such file or directory)')):
            self.write(10, raw + '\n' + terminal())
            with self.assertRaisesRegex(ValueError, 'exact product exec'):
                self.inspect()

    def test_unknown_trace_record_truncated_argv_and_namespace_change_fail_closed(self):
        for raw, error in ((exec_line(self.command) + '\nstrace: Process 10 detached', 'unparsed'),
                           (exec_line(self.command).replace('"mcp",', '..., "mcp",'), 'truncated'),
                           (exec_line(self.command) + '\nunshare(CLONE_NEWPID) = 0', 'namespace')):
            self.write(10, raw + '\n' + terminal())
            with self.assertRaisesRegex(ValueError, error):
                self.inspect()

    def test_unknown_selected_syscall_is_never_silently_ignored(self):
        self.write(10, exec_line(self.command) + '\nfuture_network_call(0) = -1 EPERM (Operation not permitted)\n' + terminal())
        self.assertEqual(self.inspect()['attempts'][0]['syscall'], 'future_network_call')

    def test_only_successful_anonymous_stream_ipc_is_separately_accounted(self):
        self.write(10, exec_line(self.command)
                   + '\nsocketpair(AF_UNIX, SOCK_STREAM|SOCK_CLOEXEC|SOCK_NONBLOCK, 0, '
                   '[3<UNIX-STREAM:[101->102]>, 4<UNIX-STREAM:[102->101]>]) = 0\n'
                   + terminal())
        report = self.inspect()
        self.assertEqual(len(report['local_anonymous_ipc']), 1)
        self.assertEqual(report['attempts'], [])
        for call in ('socketpair(AF_INET, SOCK_STREAM, 0, [3, 4]) = 0',
                     'socketpair(AF_UNIX, SOCK_DGRAM, 0, [3, 4]) = 0',
                     'socketpair(AF_UNIX, SOCK_STREAM, 1, [3, 4]) = 0',
                     'socketpair(AF_UNIX, SOCK_STREAM, 0, [3, 4]) = -1 EPERM (Operation not permitted)',
                     'socket(AF_UNIX, SOCK_STREAM, 0) = 3',
                     'connect(3, {sa_family=AF_UNIX, sun_path="/tmp/service"}, 22) = 0',
                     'recvfrom(3, "reply", 5, 0, NULL, NULL) = 5'):
            with self.subTest(call=call):
                self.write(10, exec_line(self.command) + '\n' + call + '\n' + terminal())
                self.assertTrue(self.inspect()['attempts'])

    def test_seqpacket_receive_is_bound_to_the_actual_anonymous_pair_inode(self):
        self.write(10, '100.000001 ' + exec_line(self.command)
                   + '\n100.000002 socketpair(AF_UNIX, SOCK_SEQPACKET|SOCK_CLOEXEC, 0, '
                   '[14<UNIX-SEQPACKET:[101->102]>, 15<UNIX-SEQPACKET:[102->101]>]) = 0\n'
                   '100.000003 recvfrom(14<UNIX-SEQPACKET:[101]>, "", 8, 0, NULL, NULL) = 0\n'
                   '100.000004 exit_group(0) = ?\n100.000005 +++ exited with 0 +++\n')
        report = self.inspect()
        self.assertEqual(report['attempts'], [])
        self.assertEqual(len(report['local_anonymous_ipc']), 2)
        receipt = report['local_anonymous_ipc'][1]
        self.assertEqual(receipt['endpoint_inode'], 101)
        self.assertEqual(receipt['pair_creator_pid'], 10)
        self.assertEqual(receipt['pair_creation_line'], 2)

    def test_inode_proof_survives_fork_and_fd_duplication_without_assuming_fd_numbers(self):
        self.write(10, '100.000001 ' + exec_line(self.command)
                   + '\n100.000002 socketpair(AF_UNIX, SOCK_SEQPACKET, 0, '
                   '[3<UNIX:[101->102]>, 4<UNIX:[102->101]>]) = 0\n'
                   '100.000003 fork() = 11\n100.000020 exit_group(0) = ?\n'
                   '100.000021 +++ exited with 0 +++\n')
        self.write(11, '100.000004 recvfrom(9<UNIX:[102->101]>, "12345678", 8, 0, NULL, NULL) = 8\n'
                   '100.000005 exit_group(0) = ?\n100.000006 +++ exited with 0 +++\n')
        report = self.inspect()
        self.assertEqual(report['attempts'], [])
        received = [row for row in report['local_anonymous_ipc'] if row.get('endpoint_inode')]
        self.assertEqual(len(received), 1)
        self.assertEqual(received[0]['pair_creator_pid'], 10)

    def test_unknown_reused_future_or_addressed_pair_fds_never_become_zero_attempts(self):
        base = ('100.000001 ' + exec_line(self.command)
                + '\n100.000002 socketpair(AF_UNIX, SOCK_SEQPACKET, 0, '
                '[3<UNIX:[101->102]>, 4<UNIX:[102->101]>]) = 0\n')
        calls = (
            'recvfrom(3, "", 8, 0, NULL, NULL) = 0',
            'recvfrom(3<UNIX:[999->1000]>, "", 8, 0, NULL, NULL) = 0',
            'recvfrom(3<TCP:[101]>, "", 8, 0, NULL, NULL) = 0',
            'recvfrom(3<UNIX:[101->999]>, "", 8, 0, NULL, NULL) = 0',
            'recvfrom(3<UNIX:[101->102,"/tmp/service"]>, "", 8, 0, NULL, NULL) = 0',
            'recvfrom(3<UNIX:[101]>, "", 8, 0, {sa_family=AF_UNIX}, NULL) = 0',
            'recvfrom(3<UNIX:[101]>, "", 8, MSG_DONTWAIT, NULL, NULL) = 0',
            'recvfrom(3<UNIX:[101]>, "", 7, 0, NULL, NULL) = 0',
            'sendto(3<UNIX:[101]>, "x", 1, 0, NULL, 0) = 1',
            'recvmsg(3<UNIX:[101]>, {msg_control=[{cmsg_type=SCM_RIGHTS}]}, 0) = 1',
            'pidfd_getfd(7, 3, 0) = 4',
        )
        for call in calls:
            with self.subTest(call=call):
                self.write(10, base + '100.000003 ' + call
                           + '\n100.000004 exit_group(0) = ?\n100.000005 +++ exited with 0 +++\n')
                self.assertTrue(self.inspect()['attempts'])
        # A pair in an unrelated runner branch is not a product-tree witness.
        self.write(10, '100.000003 ' + exec_line(self.command)
                   + '\n100.000004 recvfrom(3<UNIX:[101]>, "", 8, 0, NULL, NULL) = 0\n'
                   '100.000005 +++ exited with 0 +++\n')
        self.write(1, '100.000001 socketpair(AF_UNIX, SOCK_SEQPACKET, 0, '
                   '[3<UNIX:[101->102]>, 4<UNIX:[102->101]>]) = 0\n'
                   '100.000002 fork() = 10\n100.000006 +++ exited with 0 +++\n')
        self.assertTrue(self.inspect()['attempts'])

    def test_pair_created_later_or_without_timestamp_is_not_a_receive_witness(self):
        for timestamp in ('100.000009 ', ''):
            with self.subTest(timestamp=timestamp):
                self.write(10, '100.000001 ' + exec_line(self.command)
                           + '\n100.000002 fork() = 11\n'
                           + timestamp + 'socketpair(AF_UNIX, SOCK_SEQPACKET, 0, '
                           '[3<UNIX:[101->102]>, 4<UNIX:[102->101]>]) = 0\n'
                           '100.000010 +++ exited with 0 +++\n')
                self.write(11, '100.000003 recvfrom(3<UNIX:[101]>, "", 8, 0, NULL, NULL) = 0\n'
                           '100.000011 +++ exited with 0 +++\n')
                self.assertTrue(self.inspect()['attempts'])

    def test_production_trace_requires_timestamps_and_ambiguous_inode_reuse_is_rejected(self):
        self.write(10, exec_line(self.command) + '\n' + terminal())
        with self.assertRaisesRegex(ValueError, 'missing trace timestamp'):
            trace.read_traces(self.prefix, require_timestamps=True)
        pair = ('socketpair(AF_UNIX, SOCK_SEQPACKET, 0, '
                '[3<UNIX:[101->102]>, 4<UNIX:[102->101]>]) = 0\n')
        self.write(10, '100.000001 ' + exec_line(self.command)
                   + '\n100.000002 ' + pair + '100.000003 ' + pair
                   + '100.000004 +++ exited with 0 +++\n')
        with self.assertRaisesRegex(ValueError, 'ambiguous socket inode'):
            self.inspect()

    def synthetic_matrix(self):
        cases, root_trace = [], exec_line(['/fixture/runner']) + '\n'
        def guard(pid, command):
            return {'action': 'kill', 'result': 'filter_loaded_before_exec', 'exec_command': command,
                    'exec_sha256': 'digest', 'socket_fds_before_exec': [],
                    'before': {'NSpid': str(pid), 'Pid': str(pid), 'NoNewPrivs': '1',
                               'Seccomp': '2', 'Seccomp_filters': '1'},
                    'after': {'NSpid': str(pid), 'Pid': str(pid), 'NoNewPrivs': '1',
                              'Seccomp': '2', 'Seccomp_filters': '2'}}
        for index, disabled in enumerate((False, True)):
            case = {'explicitly_disabled': disabled, 'tools_listed': sorted(trace.TOOLS),
                    'tools_executed': sorted(trace.TOOLS), 'local_source_verified': True,
                    'error_contracts_verified': True, 'reopen_and_persisted_adr_verified': True,
                    'semantic_cache_absent': True}
            for offset, prefix in ((0, 'child'), (10, 'reopened_child')):
                pid = 10 + index * 20 + offset
                root_trace += f'fork() = {pid}\n'
                self.write(pid, exec_line(self.command) + '\n' + terminal())
                proof = guard(pid, self.command)
                security = dict(proof['after'], RequestedPid=str(pid), ProcPid=str(pid),
                                identity='same_pid_namespace_and_direct_parent')
                case['child_network_guard' if prefix == 'child' else 'reopened_network_guard'] = proof
                case[prefix + '_security'] = security
                case['child_exit' if prefix == 'child' else 'reopened_exit'] = {'exit_code': 0, 'success': True}
            cases.append(case)
        probes = {}
        for pid, family, af in ((50, 'ipv4', 'AF_INET'), (60, 'ipv6', 'AF_INET6')):
            command = ['/fixture/python', '-c', 'socket-probe-' + family]
            root_trace += f'fork() = {pid}\n'
            self.write(pid, exec_line(command) + f'\nsocket({af}, SOCK_STREAM, 0) = ?\n+++ killed by SIGSYS +++\n')
            probes[family] = {'reached_socket': True, 'exit_code': -31, 'pre_exec': guard(pid, command)}
        self.write(1, root_trace + terminal())
        return {'binary': self.command[0], 'build_receipt': {'binary_sha256': 'digest'},
                'cases': cases, 'network_positive_controls': {'probes': probes}}

    def test_standalone_guard_accepts_a_real_filter_increment_from_bare_host_zero(self):
        proof = self.synthetic_matrix()['cases'][0]['child_network_guard']
        proof['before'].update(Seccomp='0', Seccomp_filters='0', NoNewPrivs='0')
        proof['after']['Seccomp_filters'] = '1'
        self.assertEqual(trace.guard_root(proof, require_parent_policy=False), 10)

    def test_guard_receipt_requires_its_own_filter_increment_and_same_process(self):
        for field, value in (('Seccomp_filters', '2'), ('Seccomp_filters', '-1'),
                             ('NSpid', '900'), ('Pid', '900')):
            with self.subTest(field=field, value=value):
                proof = self.synthetic_matrix()['cases'][0]['child_network_guard']
                proof['before'][field] = value
                with self.assertRaisesRegex(ValueError, 'filter increment|guard PID'):
                    trace.guard_root(proof)

    def test_product_and_positive_probe_cannot_omit_inherited_parent_isolation(self):
        for kind in ('product', 'probe'):
            with self.subTest(kind=kind):
                observations = self.synthetic_matrix()
                proof = (observations['cases'][0]['child_network_guard'] if kind == 'product' else
                         observations['network_positive_controls']['probes']['ipv4']['pre_exec'])
                proof['before'].update(Seccomp='0', Seccomp_filters='0', NoNewPrivs='0')
                proof['after']['Seccomp_filters'] = '1'
                with self.assertRaisesRegex(ValueError, 'inherited parent isolation'):
                    trace.verify_product_observations(observations, trace.read_traces(self.prefix), 'digest')

    def test_full_matrix_requires_four_disjoint_product_trees_and_observed_probe_calls(self):
        observations = self.synthetic_matrix()
        report = trace.verify_product_observations(observations, trace.read_traces(self.prefix), 'digest')
        self.assertEqual(report['network_socket_attempts'], 0)
        self.assertEqual(len(report['product_trees']), 4)
        self.assertEqual(len(report['positive_controls_outside_product_trees']), 2)
        self.write(10, exec_line(self.command) + '\nfork() = 11\n' + terminal())
        self.write(11, 'socket(AF_INET, SOCK_STREAM, 0) = ?\n+++ killed by SIGSYS +++\n')
        report = trace.verify_product_observations(observations, trace.read_traces(self.prefix), 'digest')
        self.assertEqual(report['status'], 'failed')
        self.assertIsNone(report['network_socket_attempts'])

    def test_incomplete_tool_contract_wrong_guard_pid_and_missing_positive_trace_are_rejected(self):
        observations = self.synthetic_matrix()
        observations['cases'][0]['tools_executed'].remove('search')
        with self.assertRaisesRegex(ValueError, 'fourteen tools'):
            trace.verify_product_observations(observations, trace.read_traces(self.prefix), 'digest')
        observations = self.synthetic_matrix()
        observations['cases'][0]['child_security']['RequestedPid'] = '900'
        with self.assertRaisesRegex(ValueError, 'identity disagreement'):
            trace.verify_product_observations(observations, trace.read_traces(self.prefix), 'digest')
        observations = self.synthetic_matrix()
        command = observations['network_positive_controls']['probes']['ipv4']['pre_exec']['exec_command']
        self.write(50, exec_line(command) + '\n+++ killed by SIGSYS +++\n')
        with self.assertRaisesRegex(ValueError, 'actual positive socket probe'):
            trace.verify_product_observations(observations, trace.read_traces(self.prefix), 'digest')

    def test_positive_controls_cannot_reuse_a_tree_or_claim_the_wrong_address_family(self):
        observations = self.synthetic_matrix()
        probes = observations['network_positive_controls']['probes']
        probes['ipv6'] = probes['ipv4']
        with self.assertRaisesRegex(ValueError, 'positive probe trees overlap'):
            trace.verify_product_observations(observations, trace.read_traces(self.prefix), 'digest')
        observations = self.synthetic_matrix()
        path = self.root / 'syscalls.60'
        path.write_text(path.read_text().replace('socket(AF_INET6, ', 'socket(AF_INET, '))
        with self.assertRaisesRegex(ValueError, 'exact address family'):
            trace.verify_product_observations(observations, trace.read_traces(self.prefix), 'digest')


if __name__ == '__main__':
    unittest.main()
