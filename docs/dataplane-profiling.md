## Dataplane Profiling Guide
1. Start profiling

Enable profiling in Helm by switching the image to the dev tag and turning on the profiling flag:

```bash
helm upgrade argon . \
--set image.tag=dev \
--set dataplane.pprofEnabled=true
```

This will deploy the profiling-enabled container.
perf record will write its output to:

/var/argon/pprof/perf.data

2. Stop profiling

Once you’ve collected enough samples, disable profiling again:
```bash
helm upgrade argon . \
--set image.tag=dev \
--set dataplane.pprofEnabled=false
```


3. Convert perf.data → flamegraph.svg

Make sure your host allows perf:

```bash
# Allow perf to access kernel events
echo -1 | sudo tee /proc/sys/kernel/perf_event_paranoid

# Sometimes also needed:
echo 0 | sudo tee /proc/sys/kernel/kptr_restrict
```

```bash
flamegraph --perfdata /out/perf.data -o /out/flamegraph.svg
```

