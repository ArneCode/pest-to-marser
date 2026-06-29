import {
  createPestEditor,
  createRustEditor,
  fitEditors,
  setEditorContent,
} from "./editors.js";
import { EXAMPLES, DEFAULT_PEST } from "./examples.js";
import {
  copyText,
  downloadGrammarRs,
  downloadProjectZip,
  copyShareLink,
  initFileImport,
} from "./actions.js";
import { decodeShareState, currentShareHash } from "./share.js";
import {
  initOnboarding,
  initPaneResizer,
  updateRuleDatalist,
  updateEntryRuleHint,
  renderErrors,
  clearErrors,
  errorsAsText,
  setShareOutdated,
  setPestFilename,
  setStatus,
  setParseDiagnostic,
  setExampleCaption,
  parseDiagnosticExtensions,
} from "./ui.js";

const STORAGE_KEY_PEST = "pest-to-marser.pest";
const STORAGE_KEY_ENTRY = "pest-to-marser.entry-rule";
const STORAGE_KEY_EMIT_COMMENTS = "pest-to-marser.emit-comments";
const STORAGE_KEY_EMIT_TRACE = "pest-to-marser.emit-trace";

function loadSaved(key) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function save(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // ignore
  }
}

function loadBool(key, fallback) {
  const v = loadSaved(key);
  if (v === "1") return true;
  if (v === "0") return false;
  return fallback;
}

function initialDoc(key, fallback) {
  const saved = loadSaved(key);
  if (saved != null && saved.length > 0) return saved;
  return fallback;
}

const entryRuleEl = document.getElementById("entry-rule");
const examplesSelect = document.getElementById("examples-select");
const emitCommentsEl = document.getElementById("emit-comments");
const emitTraceEl = document.getElementById("emit-trace");

let debounceTimer = null;
let convertFn = null;
let listRulesFn = null;
let lastShareHash = "";
let lastRawOutput = "";
let lastErrors = [];
let lastConvertMs = null;
let ruleNames = [];

const shared = decodeShareState(window.location.hash);
const savedPest = shared?.pest ?? initialDoc(STORAGE_KEY_PEST, DEFAULT_PEST);
const savedEntry =
  shared?.entryRule ?? loadSaved(STORAGE_KEY_ENTRY) ?? EXAMPLES.simple.entryRule;

if (entryRuleEl) {
  entryRuleEl.value = savedEntry;
}

if (emitCommentsEl) {
  emitCommentsEl.checked = loadBool(STORAGE_KEY_EMIT_COMMENTS, true);
}

if (emitTraceEl) {
  emitTraceEl.checked = loadBool(STORAGE_KEY_EMIT_TRACE, false);
}

function getPestSource() {
  return pestEditor.state.doc.toString();
}

function getEntryRule() {
  return entryRuleEl?.value ?? "";
}

function getEmitComments() {
  return emitCommentsEl?.checked ?? true;
}

function getEmitTrace() {
  return emitTraceEl?.checked ?? false;
}

function updateRustPane() {
  setEditorContent(rustEditor, lastRawOutput);
  const copyRustBtn = document.getElementById("copy-rust-btn");
  const downloadRsBtn = document.getElementById("download-rs-btn");
  const downloadProjectBtn = document.getElementById("download-project-btn");
  const disabled = !lastRawOutput || lastErrors.length > 0;
  if (copyRustBtn) copyRustBtn.disabled = disabled;
  if (downloadRsBtn) downloadRsBtn.disabled = disabled;
  if (downloadProjectBtn) downloadProjectBtn.disabled = disabled;
}

function markShareState() {
  const hash = currentShareHash({
    pest: getPestSource(),
    entryRule: getEntryRule(),
  });
  setShareOutdated(hash !== lastShareHash);
}

function scheduleConvert() {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(runConvert, 300);
  markShareState();
}

function wasmErrors(err) {
  if (Array.isArray(err)) {
    return err.map((item) => {
      if (typeof item === "string") return { message: item };
      if (item && typeof item.message === "string") {
        return {
          message: item.message,
          from: typeof item.from === "number" ? item.from : undefined,
          to: typeof item.to === "number" ? item.to : undefined,
        };
      }
      return { message: String(item) };
    });
  }
  if (err && typeof err === "object") {
    if (Array.isArray(err.value)) return wasmErrors(err.value);
    if (typeof err.message === "string") return [{ message: err.message }];
  }
  return [{ message: String(err) }];
}

function refreshRules() {
  if (!listRulesFn) return;
  try {
    ruleNames = listRulesFn(getPestSource());
    updateRuleDatalist(ruleNames);
    updateEntryRuleHint(getEntryRule(), ruleNames);
  } catch (err) {
    ruleNames = [];
    updateRuleDatalist([]);
    updateEntryRuleHint(getEntryRule(), []);
  }
}

function runConvert() {
  if (!convertFn) return;

  const source = getPestSource();
  const entry = getEntryRule().trim();
  const emitComments = getEmitComments();
  const emitTrace = getEmitTrace();

  refreshRules();

  const t0 = performance.now();
  try {
    const code = convertFn(source, entry, emitComments, emitTrace);
    lastConvertMs = performance.now() - t0;
    lastRawOutput = code;
    lastErrors = [];
    setEditorContent(rustEditor, code);
    clearErrors();
    setParseDiagnostic(pestEditor, null);
    setStatus(`OK · ${Math.round(lastConvertMs)}ms`, "#4ec9b0");
    updateRustPane();
  } catch (err) {
    lastConvertMs = performance.now() - t0;
    lastRawOutput = "";
    lastErrors = wasmErrors(err);
    setEditorContent(rustEditor, "");
    renderErrors(lastErrors);
    const diagnostic =
      lastErrors.find(
        (e) => typeof e.from === "number" && typeof e.to === "number",
      ) ?? null;
    setParseDiagnostic(pestEditor, diagnostic);
    setStatus(`Error · ${Math.round(lastConvertMs)}ms`, "#f48771");
    updateRustPane();
  }
  markShareState();
}

const pestEditor = createPestEditor(
  document.getElementById("pest-editor"),
  savedPest,
  (text) => {
    save(STORAGE_KEY_PEST, text);
    scheduleConvert();
  },
  parseDiagnosticExtensions(() => getPestSource()),
);

const rustEditor = createRustEditor(document.getElementById("rust-editor"));

entryRuleEl?.addEventListener("input", () => {
  save(STORAGE_KEY_ENTRY, entryRuleEl.value);
  updateEntryRuleHint(getEntryRule(), ruleNames);
  scheduleConvert();
});

emitCommentsEl?.addEventListener("change", () => {
  save(STORAGE_KEY_EMIT_COMMENTS, emitCommentsEl.checked ? "1" : "0");
  scheduleConvert();
});

emitTraceEl?.addEventListener("change", () => {
  save(STORAGE_KEY_EMIT_TRACE, emitTraceEl.checked ? "1" : "0");
  scheduleConvert();
});

examplesSelect?.addEventListener("change", () => {
  const key = examplesSelect.value;
  if (!key || !EXAMPLES[key]) return;
  const ex = EXAMPLES[key];
  setEditorContent(pestEditor, ex.pest);
  save(STORAGE_KEY_PEST, ex.pest);
  if (entryRuleEl) {
    entryRuleEl.value = ex.entryRule;
    save(STORAGE_KEY_ENTRY, ex.entryRule);
  }
  setPestFilename(null);
  setExampleCaption(ex.description ? `Example: ${ex.label} — ${ex.description}` : null);
  examplesSelect.value = "";
  scheduleConvert();
});

document.getElementById("copy-pest-btn")?.addEventListener("click", (e) => {
  copyText(getPestSource(), e.currentTarget);
});

document.getElementById("copy-rust-btn")?.addEventListener("click", (e) => {
  copyText(rustEditor.state.doc.toString(), e.currentTarget);
});

document.getElementById("copy-errors-btn")?.addEventListener("click", (e) => {
  copyText(errorsAsText(lastErrors), e.currentTarget);
});

document.getElementById("download-rs-btn")?.addEventListener("click", () => {
  downloadGrammarRs(rustEditor.state.doc.toString());
});

document.getElementById("download-project-btn")?.addEventListener("click", () => {
  if (!lastRawOutput || lastErrors.length > 0) return;
  downloadProjectZip({
    pestSource: getPestSource(),
    grammarRs: lastRawOutput,
    entryRule: getEntryRule(),
    emitTrace: getEmitTrace(),
  });
});

document.getElementById("share-link-btn")?.addEventListener("click", (e) => {
  copyShareLink(
    { pest: getPestSource(), entryRule: getEntryRule() },
    e.currentTarget,
  ).then(() => {
    lastShareHash = currentShareHash({
      pest: getPestSource(),
      entryRule: getEntryRule(),
    });
    setShareOutdated(false);
  });
});

initFileImport({
  onOpen: (text, filename) => {
    setEditorContent(pestEditor, text);
    save(STORAGE_KEY_PEST, text);
    setPestFilename(filename);
    setExampleCaption(null);
    scheduleConvert();
  },
});

function onResize() {
  fitEditors(pestEditor, rustEditor);
}

window.addEventListener("resize", onResize);
initPaneResizer(onResize);
initOnboarding();
requestAnimationFrame(onResize);

async function initWasm() {
  try {
    const wasm = await import("./pkg/web.js");
    await wasm.default();
    convertFn = wasm.convert;
    listRulesFn = wasm.list_rules;
    setStatus("Ready", "#666");
    lastShareHash = currentShareHash({
      pest: getPestSource(),
      entryRule: getEntryRule(),
    });
    setShareOutdated(false);
    runConvert();
  } catch (err) {
    setStatus("WASM load failed", "#f48771");
    renderErrors([String(err)]);
  }
}

initWasm();
