// Entry point injected into a generated crate to turn it into a benchmark runner.
//
// Usage:
//   <bin> list          -> print the case names this crate can run, one per line
//   <bin> run <case>    -> run one case and print a single JSON object on stdout
//   <bin> baseline      -> run no work; report the process footprint alone
//
// One case per process, deliberately: peak RSS is a process-lifetime high-water
// mark, so running several cases in one process would attribute the largest
// case's memory to all of them. The Node runner has the same one-case-per-process
// rule for the same reason.

fn json_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            c => vec![c],
        })
        .collect()
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let cmd = argv.get(1).map(String::as_str).unwrap_or("list");

    if cmd == "list" {
        for case in smelt_bench_cases::CASES {
            println!("{case}");
        }
        return;
    }

    if cmd == "baseline" {
        // A process that loads the whole generated library and does nothing else.
        // Subtracting this from a case's peak RSS separates "what the runtime costs
        // to exist" from "what the workload allocated".
        let (user, sys) = smelt_bench_harness::cpu_seconds();
        println!(
            "{{\"kind\":\"baseline\",\"peak_rss_bytes\":{},\"rss_bytes\":{},\"cpu_user_s\":{},\"cpu_sys_s\":{}}}",
            smelt_bench_harness::peak_rss_bytes(),
            smelt_bench_harness::current_rss_bytes(),
            user,
            sys
        );
        return;
    }

    let case = argv.get(2).cloned().unwrap_or_default();
    let Some(m) = smelt_bench_cases::run_case(&case) else {
        eprintln!("unknown case: {case}");
        std::process::exit(2);
    };
    let (user, sys) = smelt_bench_harness::cpu_seconds();
    let (lists, promises, functions) = smelt_bench_harness::identity_table_sizes();
    println!(
        "{{\"kind\":\"result\",\"case\":\"{}\",\"ns_per_op_median\":{},\"ns_per_op_best\":{},\
\"ops_per_sec\":{},\"samples\":{},\"iterations\":{},\"checksum\":{},\
\"peak_rss_bytes\":{},\"rss_bytes\":{},\"cpu_user_s\":{},\"cpu_sys_s\":{},\
\"identity_lists\":{},\"identity_promises\":{},\"identity_functions\":{}}}",
        json_escape(&case),
        m.ns_per_op_median,
        m.ns_per_op_best,
        m.ops_per_sec,
        m.samples,
        m.iterations,
        m.checksum,
        smelt_bench_harness::peak_rss_bytes(),
        smelt_bench_harness::current_rss_bytes(),
        user,
        sys,
        lists,
        promises,
        functions
    );
}
