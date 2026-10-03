    #[cfg(feature = "semantic-http")]
    #[derive(Default)]
    struct BoundedHold {
        state: Mutex<(usize, bool)>,
        wake: std::sync::Condvar,
    }
    #[cfg(feature = "semantic-http")]
    impl BoundedHold {
        fn enter(&self) {
            let mut state = self.state.lock().unwrap();
            state.0 += 1;
            self.wake.notify_all();
            while !state.1 {
                let (next, timeout) = self
                    .wake
                    .wait_timeout(state, std::time::Duration::from_secs(10))
                    .unwrap();
                state = next;
                assert!(!timeout.timed_out(), "finite synthetic hold exceeded");
            }
        }
        fn wait(&self, n: usize) {
            let mut state = self.state.lock().unwrap();
            while state.0 < n {
                let (next, timeout) = self
                    .wake
                    .wait_timeout(state, std::time::Duration::from_secs(10))
                    .unwrap();
                state = next;
                assert!(
                    !timeout.timed_out(),
                    "parallel provider calls did not enter"
                );
            }
        }
        fn release(&self) {
            self.state.lock().unwrap().1 = true;
            self.wake.notify_all();
        }
    }
    #[cfg(feature = "semantic-http")]
    struct BoundedRelease(Arc<BoundedHold>);
    #[cfg(feature = "semantic-http")]
    impl Drop for BoundedRelease {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    #[cfg(feature = "semantic-http")]
    struct BoundedProvider {
        fake: FakeProvider,
        hold: Arc<BoundedHold>,
    }
    #[cfg(feature = "semantic-http")]
    impl EmbeddingProvider for BoundedProvider {
        fn space(&self) -> &cc_semantic::types::VectorSpace {
            self.fake.space()
        }
        fn embed_documents(
            &self,
            inputs: &[DocumentInput],
        ) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.hold.enter();
            self.fake.embed_documents(inputs)
        }
        fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.hold.enter();
            self.fake.embed_queries(inputs)
        }
    }
    /// Run alone in a fresh test process. A normal unlimited project assembly
    /// initializes the health facade's lazy permissive shared gate; subsequent
    /// explicit 4+2 project assembly keeps that same permissive gate. This is
    /// retained counterexample evidence, not a passing limit-contract assertion.
    #[cfg(feature = "semantic-http")]
    #[test]
    #[ignore = "retained first-wins counterexample; run alone in a fresh test process"]
    fn bounded_parallel_existing_first_wins_counterexample() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut config = ProjectConfig::default();
        config.semantic.enabled = true;
        config.semantic.model_id = "fake/first-wins-counterexample".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        let lookup = |name: &str| {
            (name == cc_semantic::cache::CACHE_ROOT_ENV).then(|| {
                dir.path()
                    .join("owned-cache")
                    .to_string_lossy()
                    .into_owned()
            })
        };
        let db = Arc::new(IndexDb::open(&dir.path().join("first.db")).unwrap().0);
        let first =
            semantic_wiring::assemble_with("normal-first-unlimited", &config, db, lookup, false)
                .unwrap()
                .unwrap();
        let shared = crate::service_factory::semantic_provider_gate();
        assert_eq!(shared.snapshot().max_concurrent, usize::MAX);
        assert_eq!(shared.snapshot().max_concurrent_per_project, None);
        config.semantic.max_concurrent = 4;
        config.semantic.max_concurrent_per_project = 2;
        let db = Arc::new(IndexDb::open(&dir.path().join("second.db")).unwrap().0);
        semantic_wiring::assemble_with("normal-second-explicit", &config, db, lookup, false)
            .unwrap()
            .unwrap();
        let effective = crate::service_factory::semantic_provider_gate();
        assert!(Arc::ptr_eq(&shared, &effective));
        assert_eq!(
            effective.snapshot().max_concurrent,
            usize::MAX,
            "existing first-wins preserves permissive gate despite requested global4"
        );
        assert_eq!(
            effective.snapshot().max_concurrent_per_project,
            None,
            "existing first-wins preserves no project cap despite requested project2"
        );
        let hold = Arc::new(BoundedHold::default());
        let _release = BoundedRelease(hold.clone());
        let provider = Arc::new(BoundedProvider {
            fake: FakeProvider::new(FakeProviderConfig::new(first.space.clone())),
            hold: hold.clone(),
        });
        let input =
            DocumentInput::from_bytes(b"normal synthetic shared-gate counterexample").unwrap();
        std::thread::scope(|scope| {
            let workers: Vec<_> = [
                ("explicit-b", false),
                ("explicit-b", false),
                ("explicit-b", true),
                ("explicit-c", false),
                ("explicit-c", false),
            ]
            .into_iter()
            .map(|(project, query)| {
                let admitted = AdmittedProvider {
                    inner: provider.clone(),
                    gate: Some(crate::service_factory::semantic_provider_gate()),
                    namespace: project.into(),
                    wait: std::time::Duration::from_millis(100),
                    cancellation: tokio_util::sync::CancellationToken::new(),
                    control: None,
                };
                let input = input.clone();
                scope.spawn(move || {
                    if query {
                        admitted.embed_queries(&[QueryInput::from_bytes(
                            b"finite query alongside two bounded document attempts",
                        )
                        .unwrap()])
                    } else {
                        admitted.embed_documents(&[input])
                    }
                })
            })
            .collect();
            hold.wait(5);
            let snapshot = effective.snapshot();
            assert_eq!(
                snapshot.in_flight, 5,
                "actual decorated calls exceed requested global4"
            );
            assert_eq!(
                snapshot.per_project_in_flight["explicit-b"], 3,
                "actual decorated calls exceed requested project2"
            );
            println!("FIRST_WINS_COUNTEREXAMPLE requested_global=4 requested_project=2 effective_global={} effective_project={:?} decorated_provider_active=5 explicit_b_active=3 b_document_attempts=2 b_queries=1 c_document_attempts=2",snapshot.max_concurrent,snapshot.max_concurrent_per_project);
            hold.release();
            for worker in workers {
                worker.join().unwrap().unwrap();
            }
        });
        assert_eq!(effective.snapshot().in_flight, 0);
    }