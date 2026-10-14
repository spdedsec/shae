// Shae Showcase Website — Interactive Logic
// Fast, lightweight, zero-framework vanilla TypeScript

interface SnippetData {
  filename: string;
  codeHtml: string;
  command: string;
  output: string;
}

const SNIPPETS: Record<string, SnippetData> = {
  server: {
    filename: "web_server.shae",
    codeHtml: `<span class="syn-comment">// Simple, production-ready HTTP microservice in Shae</span>
<span class="syn-kw">let</span> state = { visits: <span class="syn-num">0</span> }

<span class="syn-kw">fn</span> <span class="syn-fn">handler</span>(req) {
    print(<span class="syn-str">"Received"</span>, req.method, <span class="syn-str">"request to"</span>, req.path)
    state.visits += <span class="syn-num">1</span>
    
    <span class="syn-kw">if</span> req.path == <span class="syn-str">"/"</span> {
        <span class="syn-kw">return</span> <span class="syn-str">"&lt;h1&gt;Welcome to Shae!&lt;/h1&gt;&lt;p&gt;Visits: {state.visits}&lt;/p&gt;"</span>
    } <span class="syn-kw">else if</span> req.path == <span class="syn-str">"/api"</span> {
        <span class="syn-kw">return</span> state
    }
    
    <span class="syn-kw">return</span> { <span class="syn-str">"status"</span>: <span class="syn-num">404</span>, <span class="syn-str">"body"</span>: <span class="syn-str">"&lt;h1&gt;404 Not Found&lt;/h1&gt;"</span> }
}

print(<span class="syn-str">"Starting Shae server..."</span>)
<span class="syn-fn">serve</span>(<span class="syn-num">8080</span>, handler)`,
    command: "$ shae run web_server.shae",
    output: `Starting Shae server...
Server running on http://localhost:8080
[Thread 1] Listening on 127.0.0.1:8080 (keep-alive enabled)
Received GET request to /
HTTP/1.1 200 OK (Content-Length: 52, Duration: 0.18ms)`
  },

  unicode: {
    filename: "unicode_demo.shae",
    codeHtml: `<span class="syn-comment">// First-Class UAX #29 Unicode & Extended Grapheme Clusters</span>
<span class="syn-kw">let</span> unicode = <span class="syn-kw">use</span> <span class="syn-str">"std:unicode"</span>

<span class="syn-comment">// Multi-codepoint composite emojis and flags</span>
<span class="syn-kw">let</span> family = <span class="syn-str">"👨‍👩‍👧‍👦"</span>
<span class="syn-kw">let</span> flag = <span class="syn-str">"🇺🇸"</span>
<span class="syn-kw">let</span> mixed = <span class="syn-str">"Hello, 🌍! 🚀"</span>

print(<span class="syn-str">"Visual grapheme count:"</span>, family.grapheme_len())    <span class="syn-comment">// 1 (correct!)</span>
print(<span class="syn-str">"Raw UTF-8 byte length:"</span>, family.byte_len())        <span class="syn-comment">// 25 bytes</span>
print(<span class="syn-str">"Flag graphemes:"</span>, flag.graphemes())                <span class="syn-comment">// ["🇺🇸"]</span>

<span class="syn-comment">// Safe grapheme slicing that never splits UTF-8 codepoints</span>
<span class="syn-kw">let</span> sliced = mixed.slice_graphemes(<span class="syn-num">7</span>, <span class="syn-num">10</span>)
print(<span class="syn-str">"Sliced graphemes [7..10]:"</span>, sliced)                <span class="syn-comment">// "🌍! "</span>`,
    command: "$ shae run unicode_demo.shae",
    output: `Visual grapheme count: 1
Raw UTF-8 byte length: 25
Flag graphemes: ["🇺🇸"]
Sliced graphemes [7..10]: "🌍! "`
  },

  sets: {
    filename: "collections_demo.shae",
    codeHtml: `<span class="syn-comment">// SipHash-2-4 Randomized Collections & First-Class Sets</span>
<span class="syn-kw">let</span> collections = <span class="syn-kw">use</span> <span class="syn-str">"std:collections"</span>

<span class="syn-kw">let</span> admin_roles = collections.Set([<span class="syn-str">"read"</span>, <span class="syn-str">"write"</span>, <span class="syn-str">"admin"</span>])
<span class="syn-kw">let</span> user_roles  = collections.Set([<span class="syn-str">"read"</span>, <span class="syn-str">"comment"</span>])

<span class="syn-comment">// Fast O(N) Set Algebra</span>
<span class="syn-kw">let</span> shared = admin_roles.intersect(user_roles)
<span class="syn-kw">let</span> all_perms = admin_roles.union(user_roles)

print(<span class="syn-str">"Shared roles:"</span>, shared.to_array())      <span class="syn-comment">// ["read"]</span>
print(<span class="syn-str">"All permissions:"</span>, all_perms.size())    <span class="syn-comment">// 4</span>
print(<span class="syn-str">"Has admin:"</span>, admin_roles.has(<span class="syn-str">"admin"</span>))  <span class="syn-comment">// true</span>`,
    command: "$ shae run collections_demo.shae",
    output: `Shared roles: ["read"]
All permissions: 4
Has admin: true
[Security] SipHash-2-4 randomized seeds active (Hash-DoS Immune)`
  },

  signals: {
    filename: "signals_demo.shae",
    codeHtml: `<span class="syn-comment">// Cross-Platform Graceful OS Interrupt & Shutdown Hooks</span>
<span class="syn-kw">let</span> sys = <span class="syn-kw">use</span> <span class="syn-str">"std:sys"</span>

print(<span class="syn-str">"Starting worker. Supported signals:"</span>, sys.signals())

<span class="syn-kw">let</span> running = <span class="syn-kw">true</span>

sys.on_signal(<span class="syn-str">"SIGINT"</span>, <span class="syn-kw">fn</span>() {
    print(<span class="syn-str">"Intercepted SIGINT! Cleaning up connections..."</span>)
    running = <span class="syn-kw">false</span>
})

<span class="syn-comment">// Programmatic signal trigger for deterministic unit tests</span>
sys.trigger_signal(<span class="syn-str">"SIGINT"</span>)
sys.poll_signals()

print(<span class="syn-str">"Clean shutdown completed. Running status:"</span>, running)`,
    command: "$ shae run signals_demo.shae",
    output: `Starting worker. Supported signals: ["SIGINT", "SIGTERM", "SIGHUP"]
Intercepted SIGINT! Cleaning up connections...
Clean shutdown completed. Running status: false`
  },

  data: {
    filename: "data_structures.shae",
    codeHtml: `<span class="syn-comment">// Expressive Data Structures & Maps</span>
<span class="syn-kw">let</span> inventory = [
    { id: <span class="syn-num">1</span>, name: <span class="syn-str">"Laptop"</span>, price: <span class="syn-num">1200</span> },
    { id: <span class="syn-num">2</span>, name: <span class="syn-str">"Keyboard"</span>, price: <span class="syn-num">150</span> },
    { id: <span class="syn-num">3</span>, name: <span class="syn-str">"Monitor"</span>, price: <span class="syn-num">400</span> }
]

print(<span class="syn-str">"We have"</span>, inventory.len(), <span class="syn-str">"items."</span>)

<span class="syn-kw">let</span> first = inventory[<span class="syn-num">0</span>]
print(<span class="syn-str">"First item:"</span>, first.name, <span class="syn-str">"costs $"</span> + str(first.price))

<span class="syn-comment">// Safe property navigation with fallback default</span>
<span class="syn-kw">let</span> val = first.get(<span class="syn-str">"missing_key"</span>, <span class="syn-str">"default_value"</span>)
print(<span class="syn-str">"Fallback value:"</span>, val)`,
    command: "$ shae run data_structures.shae",
    output: `We have 3 items.
First item: Laptop costs $1200
Fallback value: default_value`
  }
};

const INSTALL_COMMANDS: Record<string, string> = {
  curl: "curl -fsSL https://github.com/spdedsec/shae/releases/download/v0.3.1/shae-v0.3.1-linux-x86_64.tar.gz | tar -xz",
  win: "irm https://github.com/spdedsec/shae/releases/download/v0.3.1/shae-v0.3.1-windows-x86_64.zip -OutFile shae.zip; Expand-Archive shae.zip",
  cargo: "cargo install --git https://github.com/spdedsec/shae.git --tag v0.3.1"
};

document.addEventListener("DOMContentLoaded", () => {
  // 1. Code Carousel Tabs
  const codeTabs = document.querySelectorAll<HTMLButtonElement>(".code-tab");
  const codeDisplay = document.getElementById("codeDisplay");
  const codeFilename = document.getElementById("codeFilename");
  const terminalCmd = document.getElementById("terminalCmd");
  const terminalText = document.getElementById("terminalText");

  function switchSnippet(snippetKey: string) {
    const data = SNIPPETS[snippetKey];
    if (!data || !codeDisplay || !codeFilename || !terminalCmd || !terminalText) return;

    codeFilename.textContent = data.filename;
    codeDisplay.innerHTML = data.codeHtml;
    terminalCmd.textContent = data.command;
    terminalText.textContent = data.output;
  }

  codeTabs.forEach(tab => {
    tab.addEventListener("click", () => {
      codeTabs.forEach(t => t.classList.remove("active"));
      tab.classList.add("active");
      const snippetKey = tab.dataset.snippet || "server";
      switchSnippet(snippetKey);
    });
  });

  // Initial code load
  switchSnippet("server");

  // 2. Install Box Tabs
  const installTabs = document.querySelectorAll<HTMLButtonElement>(".install-tab");
  const installCode = document.getElementById("installCmd");

  installTabs.forEach(tab => {
    tab.addEventListener("click", () => {
      installTabs.forEach(t => t.classList.remove("active"));
      tab.classList.add("active");
      const type = tab.dataset.install || "curl";
      if (installCode && INSTALL_COMMANDS[type]) {
        installCode.textContent = INSTALL_COMMANDS[type];
      }
    });
  });

  // 3. Copy Buttons with Feedback
  function setupCopyButton(btnId: string, getText: () => string) {
    const btn = document.getElementById(btnId);
    if (!btn) return;

    btn.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(getText());
        btn.classList.add("copied");
        setTimeout(() => btn.classList.remove("copied"), 1800);
      } catch (e) {
        console.error("Clipboard copy failed", e);
      }
    });
  }

  setupCopyButton("copyInstallBtn", () => {
    return installCode ? installCode.textContent || "" : "";
  });

  setupCopyButton("copySnippetBtn", () => {
    const activeTab = document.querySelector<HTMLButtonElement>(".code-tab.active");
    const snippetKey = activeTab?.dataset.snippet || "server";
    const data = SNIPPETS[snippetKey];
    if (!data) return "";
    // strip HTML tags to get pure code
    const tempDiv = document.createElement("div");
    tempDiv.innerHTML = data.codeHtml;
    return tempDiv.textContent || tempDiv.innerText || "";
  });
});
