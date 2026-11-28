use shae::{run, run_vm};

#[test]
fn test_vdom_elements_diff_and_patch_dual_engine() {
    let script = r#"
        use { h, text, fragment, comment, diff, apply_patches, render_html } from "std:vdom";

        let old_tree = h("div", { id: "root", className: "card" }, [
            h("h1", { key: "title" }, ["Welcome"]),
            h("ul", { key: "list" }, [
                h("li", { key: "a" }, ["Alpha"]),
                h("li", { key: "b" }, ["Beta"]),
                h("li", { key: "c" }, ["Gamma"])
            ])
        ]);

        let new_tree = h("div", { id: "root", data_state: "ready" }, [
            h("h1", { key: "title" }, ["Welcome Back"]),
            h("ul", { key: "list" }, [
                h("li", { key: "c" }, ["Gamma"]),
                h("li", { key: "d" }, ["Delta"]),
                h("li", { key: "a" }, ["Alpha Updated"])
            ])
        ]);

        let patches = diff(old_tree, new_tree);
        assert(patches.len > 0, "diff must emit patches");

        let patched = apply_patches(old_tree, patches);
        let html_expected = render_html(new_tree);
        let html_patched = render_html(patched);
        assert(html_patched == html_expected, "patched HTML must match target HTML: " + html_patched);

        let escaped = render_html(h("p", { title: "a\"b&c" }, ["<script>alert(1)</script>"]));
        assert(escaped == "<p title=\"a&quot;b&amp;c\">&lt;script&gt;alert(1)&lt;/script&gt;</p>", "HTML must escape XSS payloads");

        let frag = fragment([text("Hello "), comment("marker"), h("br")]);
        assert(render_html(frag) == "Hello <!--marker--><br>", "fragment & void tag rendering");
    "#;

    let r_ast = run(script);
    assert!(r_ast.is_ok(), "AST evaluator VDOM test failed: {:?}", r_ast.err());

    let r_vm = run_vm(script);
    assert!(r_vm.is_ok(), "VM VDOM test failed: {:?}", r_vm.err());
}

#[test]
fn test_vdom_signals_computed_effects_and_batch_dual_engine() {
    let script = r#"
        use { signal, computed, effect, batch } from "std:web:vdom";

        let first = signal(2);
        let second = signal(3);
        let sum = computed(fn() {
            return first.get() + second.get();
        });
        let doubled = computed(fn() {
            return sum.get() * 2;
        });

        assert(sum.get() == 5, "initial computed sum must be 5");
        assert(doubled.get() == 10, "initial doubled must be 10");

        let observed = [];
        let eff = effect(fn() {
            observed.push(doubled.get());
        });

        assert(observed.len == 1, "effect must run immediately once");
        assert(observed[0] == 10, "initial observed value must be 10");

        first.set(5);
        assert(sum.get() == 8, "updated sum must be 8");
        assert(observed.len == 2, "effect must re-run on dependency change");
        assert(observed[1] == 16, "updated observed value must be 16");

        // Batch multiple writes into a single effect flush
        batch(fn() {
            first.set(10);
            second.set(20);
        });
        assert(sum.get() == 30, "batched sum must be 30");
        assert(observed.len == 3, "batch must coalesce into single effect execution");
        assert(observed[2] == 60, "batched observed value must be 60");

        // Dispose stops further effect updates
        assert(eff.dispose() == true, "effect dispose must succeed");
        first.set(100);
        assert(sum.get() == 120, "computed still updates after effect dispose");
        assert(observed.len == 3, "disposed effect must not run again");
    "#;

    let r_ast = run(script);
    assert!(r_ast.is_ok(), "AST evaluator signals test failed: {:?}", r_ast.err());

    let r_vm = run_vm(script);
    assert!(r_vm.is_ok(), "VM signals test failed: {:?}", r_vm.err());
}

#[test]
fn test_vdom_mount_and_event_dispatch_dual_engine() {
    let script = r#"
        use { h, signal, mount, render_html } from "std:vdom";

        let count = signal(0);
        let items = signal(["one", "two"]);

        let app = mount(fn() {
            let list_children = [];
            let cur_items = items.get();
            let i = 0;
            while i < cur_items.len {
                list_children.push(h("li", { key: cur_items[i] }, [cur_items[i]]));
                i = i + 1;
            }
            return h("div", { id: "counter-app" }, [
                h("span", { id: "val" }, ["Count: " + str(count.get())]),
                h("button", {
                    id: "inc",
                    onClick: fn() {
                        count.update(fn(c) { return c + 1; });
                    }
                }, ["+1"]),
                h("ul", {}, list_children)
            ]);
        });

        assert(app.render_count() == 1, "initial mount renders once");
        assert(app.html().contains("Count: 0"), "initial html must show Count: 0");

        // Dispatch click on button at child path [1]
        let patches1 = app.dispatch([1], "click");
        assert(patches1.len > 0, "dispatch must return VDOM patches");
        assert(app.render_count() == 2, "dispatch triggers reactive re-render");
        assert(app.html().contains("Count: 1"), "updated html must show Count: 1");

        // Mutate keyed list signal and trigger app.render()
        items.set(["two", "three", "one"]);
        let patches2 = app.render();
        assert(patches2.len > 0, "list reorder and insert must produce patches");
        assert(app.html().contains("<li>two</li><li>three</li><li>one</li>"), "reordered items rendered cleanly");

        assert(app.unmount() == true, "unmount must return true");
    "#;

    let r_ast = run(script);
    assert!(r_ast.is_ok(), "AST evaluator mount test failed: {:?}", r_ast.err());

    let r_vm = run_vm(script);
    assert!(r_vm.is_ok(), "VM mount test failed: {:?}", r_vm.err());
}
