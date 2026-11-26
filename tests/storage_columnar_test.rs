use shae::run_vm;

#[test]
fn test_shae_columnar_script_lifecycle() {
    let script = r#"
        use { create, insert, filter, aggregate, count, to_rows, close } from "std:columnar";

        // Create table
        let tbl = create({
            id: "int",
            device: "string",
            latency: "float",
            healthy: "bool"
        });

        assert(tbl > 0, "tbl handle must be positive integer");

        insert(tbl, { id: 1, device: "router-01", latency: 12.5, healthy: true });
        insert(tbl, { id: 2, device: "switch-02", latency: 45.2, healthy: false });
        insert(tbl, { id: 3, device: "router-03", latency: 8.4, healthy: true });
        insert(tbl, { id: 4, device: "gateway-01", latency: 19.1, healthy: true });

        assert(count(tbl) == 4, "row count must be 4");

        // Filter: healthy == true
        let healthy_tbl = filter(tbl, "healthy", "==", true);
        assert(count(healthy_tbl) == 3, "healthy count must be 3");

        // Aggregate over healthy devices
        let agg = aggregate(healthy_tbl, {
            total_latency: ["latency", "sum"],
            avg_latency: ["latency", "avg"],
            min_latency: ["latency", "min"],
            device_count: ["id", "count"]
        });

        assert(agg.device_count == 3, "device count must be 3");
        assert(agg.min_latency == 8.4, "min latency must be 8.4");
        assert(agg.total_latency == 40.0, "total latency should sum to 40.0");

        // Convert to rows
        let rows = to_rows(healthy_tbl);
        assert(rows.len == 3, "must convert 3 rows");
        assert(rows[0].device == "router-01", "first row device");
        assert(rows[1].device == "router-03", "second row device");

        close(tbl);
        close(healthy_tbl);
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "Columnar script execution failed: {:?}", res.err());
}

#[test]
fn test_shae_columnar_group_by_in_script() {
    let script = r#"
        use { create, insert, group_by, close } from "std:columnar";

        let tbl = create({
            emp_id: "int",
            department: "string",
            salary: "float"
        });

        insert(tbl, { emp_id: 101, department: "Engineering", salary: 120000.0 });
        insert(tbl, { emp_id: 102, department: "Marketing", salary: 85000.0 });
        insert(tbl, { emp_id: 103, department: "Engineering", salary: 140000.0 });
        insert(tbl, { emp_id: 104, department: "Marketing", salary: 95000.0 });
        insert(tbl, { emp_id: 105, department: "Engineering", salary: 160000.0 });

        let grouped = group_by(tbl, "department", {
            avg_salary: ["salary", "avg"],
            total_salary: ["salary", "sum"],
            headcount: ["emp_id", "count"]
        });

        assert(grouped.len == 2, "must have 2 departments");

        let i = 0;
        let eng_found = false;
        let mkt_found = false;

        while i < grouped.len {
            let g = grouped[i];
            if g.department == "Engineering" {
                eng_found = true;
                assert(g.headcount == 3, "eng headcount must be 3");
                assert(g.avg_salary == 140000.0, "eng avg salary");
                assert(g.total_salary == 420000.0, "eng total salary");
            }
            if g.department == "Marketing" {
                mkt_found = true;
                assert(g.headcount == 2, "mkt headcount must be 2");
                assert(g.avg_salary == 90000.0, "mkt avg salary");
                assert(g.total_salary == 180000.0, "mkt total salary");
            }
            i = i + 1;
        }

        assert(eng_found, "Engineering group must be found");
        assert(mkt_found, "Marketing group must be found");

        close(tbl);
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "Columnar group-by script execution failed: {:?}", res.err());
}
