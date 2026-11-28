use shae::{run, run_vm};

#[test]
fn test_ssr_template_and_escaping_dual_engine() {
    let script = r#"
        use { compile, render_template, escape_html } from "std:ssr";

        let tpl = compile("<main><h1>\{\{heading\}\}</h1>\{\{#if logged_in\}\}<ul>\{\{#each roles as r\}\}<li>\{\{@index\}\}:\{\{r\}\}</li>\{\{/each\}\}</ul>\{\{else\}\}<span>Guest</span>\{\{/if\}\}<article>\{\{\{safe_html\}\}\}</article></main>");

        let html = tpl.render({
            heading: "User <Profile>",
            logged_in: true,
            roles: ["admin", "dev&ops"],
            safe_html: "<p>Trusted</p>"
        });

        assert(html == "<main><h1>User &lt;Profile&gt;</h1><ul><li>0:admin</li><li>1:dev&amp;ops</li></ul><article><p>Trusted</p></article></main>", "rendered mismatch: " + html);

        let guest = render_template("\{\{#if active\}\}ON\{\{else\}\}OFF\{\{/if\}\}", { active: false });
        assert(guest == "OFF", "else branch must render OFF");
        assert(escape_html("<a&b>") == "&lt;a&amp;b&gt;", "escape_html must escape entities");
    "#;

    let r_ast = run(script);
    assert!(r_ast.is_ok(), "AST SSR template test failed: {:?}", r_ast.err());

    let r_vm = run_vm(script);
    assert!(r_vm.is_ok(), "VM SSR template test failed: {:?}", r_vm.err());
}

#[test]
fn test_ssr_vdom_document_hydration_and_streaming_dual_engine() {
    let script = r#"
        use { h } from "std:vdom";
        use { render_document, render_stream, serialize_state, suspense_fallback, suspense_resolve } from "std:web:ssr";

        let body_tree = h("div", { className: "shell" }, [
            h("h2", { key: "hdr" }, ["Dashboard"]),
            h("button", { key: "act", onClick: fn() { return 1; } }, ["Refresh"]),
            suspense_fallback("feed", "<p>Loading feed...</p>")
        ]);

        let doc = render_document({
            title: "Shae <Analytics>",
            lang: "en",
            root_id: "root",
            body: body_tree,
            state: { payload: "</script><script>alert(1)</script>", count: 42 },
            hydrate: true
        });

        assert(doc.contains("<!DOCTYPE html>"), "must start with doctype");
        assert(doc.contains("<title>Shae &lt;Analytics&gt;</title>"), "title must be escaped");
        assert(doc.contains("data-shae-hid=\"0\""), "keyed h2 must have hid 0");
        assert(doc.contains("data-shae-events=\"click\""), "button must emit event metadata");
        assert(doc.contains("<script id=\"__SHAE_DATA__\" type=\"application/json\">"), "state script tag missing");
        assert(!doc.contains("alert(1)</script>"), "JSON state must escape closing script tag");
        assert(doc.contains("__SHAE_STATE__"), "hydration runtime script missing");

        let chunks = render_stream(body_tree, { chunk_size: 24, hydrate: true });
        assert(chunks.len >= 3, "streaming must yield multiple chunks");
        let joined = chunks.join("");
        assert(joined.contains("Refresh") && joined.contains("Loading feed..."), "joined chunks preserve content");

        let resolved = suspense_resolve("feed", h("ul", {}, [h("li", {}, ["Item 1"])]));
        assert(resolved.contains("id=\"S:feed\"") && resolved.contains("<ul><li>Item 1</li></ul>"), "suspense resolution payload");
    "#;

    let r_ast = run(script);
    assert!(r_ast.is_ok(), "AST SSR document/stream test failed: {:?}", r_ast.err());

    let r_vm = run_vm(script);
    assert!(r_vm.is_ok(), "VM SSR document/stream test failed: {:?}", r_vm.err());
}

#[test]
fn test_option6_fullstack_http_vdom_ssr_end_to_end() {
    let script = r#"
        use { server, get } from "std:web:http";
        use { h, signal, mount } from "std:vdom";
        use { render_document } from "std:ssr";

        let visits = signal(1);
        let app = mount(fn() {
            return h("main", { id: "page" }, [
                h("h1", { key: "t" }, ["Shae Fullstack"]),
                h("p", { key: "v" }, ["Visits: " + str(visits.get())])
            ]);
        });

        let srv = server();
        srv.get("/", fn(req) {
            let html = render_document({
                title: "Shae SSR Live",
                body: app.tree(),
                state: { visits: visits.get() },
                hydrate: true
            });
            return {
                status: 200,
                headers: { "content-type": "text/html; charset=utf-8" },
                body: html
            };
        });
        let info = srv.listen("127.0.0.1:0");
        let resp = get("http://127.0.0.1:" + str(info.port) + "/");
        assert(resp.status == 200, "HTTP SSR response must be 200");
        assert(resp.body.contains("<!DOCTYPE html>"), "HTTP body must contain HTML5 doctype");
        assert(resp.body.contains("Visits: 1"), "HTTP body must contain rendered VDOM state");
        assert(resp.body.contains("__SHAE_DATA__"), "HTTP body must contain hydration payload");

        srv.stop();
        app.unmount();
    "#;

    let r_ast = run(script);
    assert!(r_ast.is_ok(), "AST fullstack HTTP+VDOM+SSR failed: {:?}", r_ast.err());

    let r_vm = run_vm(script);
    assert!(r_vm.is_ok(), "VM fullstack HTTP+VDOM+SSR failed: {:?}", r_vm.err());
}
