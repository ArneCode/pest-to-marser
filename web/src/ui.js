import { StateEffect, StateField } from "@codemirror/state";
import { linter, lintGutter } from "@codemirror/lint";

const setParseDiagnosticEffect = StateEffect.define();

const parseDiagnosticField = StateField.define({
  create() {
    return [];
  },
  update(value, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setParseDiagnosticEffect)) {
        return effect.value;
      }
    }
    return value;
  },
});

export function trailingInputOffset(source, errorMessage) {
  const match = /trailing input: (\d+) byte\(s\) remain unparsed/.exec(errorMessage);
  if (!match) return null;
  const remaining = parseInt(match[1], 10);
  if (Number.isNaN(remaining)) return null;
  const offset = source.length - remaining;
  if (offset < 0 || offset > source.length) return null;
  return { from: offset, to: source.length };
}

function unsupportedKeyword(errorMessage) {
  const match = /\b(PUSH_LITERAL|PUSH|POP_ALL|POP|PEEK_ALL|PEEK|DROP)\b/.exec(errorMessage);
  return match?.[1] ?? null;
}

function findKeywordRange(source, keyword, usedRanges = []) {
  if (!keyword) return null;
  const re = new RegExp(`\\b${keyword}\\b`, "g");
  for (const match of source.matchAll(re)) {
    const from = match.index ?? -1;
    if (from < 0) continue;
    const to = from + keyword.length;
    const alreadyUsed = usedRanges.some((range) => range.from === from && range.to === to);
    if (!alreadyUsed) {
      return { from, to };
    }
  }
  return null;
}

function diagnosticRange(diagnostic, source, usedRanges = []) {
  if (
    diagnostic &&
    typeof diagnostic.from === "number" &&
    typeof diagnostic.to === "number"
  ) {
    const from = diagnostic.from;
    const to = diagnostic.to > from ? diagnostic.to : Math.min(from + 1, source.length);
    return { from, to };
  }
  const message = diagnostic?.message ?? "";
  const unsupported = unsupportedKeyword(message);
  if (unsupported) {
    const range = findKeywordRange(source, unsupported, usedRanges);
    if (range) return range;
  }
  return trailingInputOffset(source, message);
}

function createParseDiagnosticExtension(getSource) {
  return linter((view) => {
    const diagnostics = view.state.field(parseDiagnosticField, false) ?? [];
    if (diagnostics.length === 0) return [];
    const source = getSource();
    const usedRanges = [];
    return diagnostics
      .filter((diagnostic) => diagnostic?.message)
      .map((diagnostic) => {
        const pos = diagnosticRange(diagnostic, source, usedRanges);
        if (!pos) return null;
        usedRanges.push(pos);
        return {
          from: pos.from,
          to: pos.to,
          severity: "error",
          message: diagnostic.message,
        };
      })
      .filter(Boolean);
  });
}

export function parseDiagnosticExtensions(getSource) {
  return [parseDiagnosticField, createParseDiagnosticExtension(getSource), lintGutter()];
}

export function setParseDiagnostic(view, diagnostic) {
  let value = [];
  if (Array.isArray(diagnostic)) {
    value = diagnostic
      .filter((item) => item && typeof item === "object" && typeof item.message === "string")
      .map((item) => ({
        message: item.message,
        from: typeof item.from === "number" ? item.from : undefined,
        to: typeof item.to === "number" ? item.to : undefined,
      }));
  } else if (typeof diagnostic === "string") {
    value = [{ message: diagnostic }];
  } else if (diagnostic?.message) {
    value = [diagnostic];
  }
  view.dispatch({
    effects: setParseDiagnosticEffect.of(value),
  });
}

export function initOnboarding() {
  initModal();
  initTour();
  initTracePopup();
}

function initTracePopup() {
  const popup = document.getElementById("trace-popup");
  const backdrop = document.getElementById("trace-popup-backdrop");
  const openBtn = document.getElementById("trace-help-btn");
  const closeBtn = document.getElementById("trace-popup-close");
  if (!popup || !openBtn) return;

  function openPopup() {
    popup.hidden = false;
  }

  function closePopup() {
    popup.hidden = true;
  }

  openBtn.addEventListener("click", openPopup);
  closeBtn?.addEventListener("click", closePopup);
  backdrop?.addEventListener("click", closePopup);

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !popup.hidden) {
      closePopup();
    }
  });
}

const DISMISS_KEY = "grammar-to-marser.intro-dismissed";

function shouldShowModal() {
  try {
    // Migrate old key
    if (localStorage.getItem("grammar-to-marser.onboarding-dismissed") === "1") {
      localStorage.setItem(DISMISS_KEY, "1");
      localStorage.removeItem("grammar-to-marser.onboarding-dismissed");
    }
    return localStorage.getItem(DISMISS_KEY) !== "1";
  } catch {
    return false;
  }
}

function initModal() {
  const modal = document.getElementById("onboarding-modal");
  const backdrop = document.getElementById("onboarding-backdrop");
  const continueBtn = document.getElementById("modal-continue");
  const tourBtn = document.getElementById("modal-tour");
  const skipCheckbox = document.getElementById("modal-skip-next");
  if (!modal) return;

  function closeModal() {
    modal.hidden = true;
    if (skipCheckbox?.checked) {
      try {
        localStorage.setItem(DISMISS_KEY, "1");
      } catch {
        // ignore
      }
    }
  }

  if (shouldShowModal()) {
    modal.hidden = false;
  }

  continueBtn?.addEventListener("click", closeModal);
  backdrop?.addEventListener("click", closeModal);

  tourBtn?.addEventListener("click", () => {
    closeModal();
    startTour({ force: true });
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !modal.hidden) {
      closeModal();
    }
  });
}

export function initDownloadDialog({ getDefaultName, onConfirm }) {
  const dialog = document.getElementById("download-dialog");
  const backdrop = document.getElementById("download-backdrop");
  const openBtn = document.getElementById("download-project-btn");
  const cancelBtn = document.getElementById("download-cancel-btn");
  const confirmBtn = document.getElementById("download-confirm-btn");
  const nameInput = document.getElementById("download-name-input");
  if (!dialog || !openBtn) return;

  function openDialog() {
    if (nameInput) nameInput.value = getDefaultName?.() ?? "grammar-parser";
    dialog.hidden = false;
    nameInput?.focus();
    nameInput?.select();
  }

  function closeDialog() {
    dialog.hidden = true;
  }

  openBtn.addEventListener("click", () => {
    if (openBtn.disabled) return;
    openDialog();
  });

  cancelBtn?.addEventListener("click", closeDialog);
  backdrop?.addEventListener("click", closeDialog);

  confirmBtn?.addEventListener("click", () => {
    const name = nameInput?.value ?? "";
    closeDialog();
    onConfirm?.(name);
  });

  nameInput?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      const name = nameInput.value ?? "";
      closeDialog();
      onConfirm?.(name);
    }
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !dialog.hidden) {
      closeDialog();
    }
  });
}

export function updateGrammarPanel(syntax) {
  const pestContent = document.getElementById("grammar-pest");
  const pegContent = document.getElementById("grammar-peg");
  if (pestContent) pestContent.hidden = syntax !== "pest";
  if (pegContent) pegContent.hidden = syntax !== "peg";
}

const TOUR_STEPS = [
  {
    targetId: "mode-switch",
    text: "Choose Pest or PEG — both grammar formats are supported. Switching modes loads the last grammar you used in that format.",
  },
  {
    targetId: "examples-select",
    text: "Load a built-in example to see a complete conversion. Each example demonstrates grammar constructs you can use in your own rules.",
  },
  {
    targetId: "grammar-editor",
    text: "Paste or edit your grammar here — the Rust parser updates live as you type. Drag a .pest or .peg file onto this area to open it. Unsupported constructs show as errors you can click to jump to.",
  },
  {
    targetId: "rust-pane",
    text: "The generated Rust parser appears here. Use Copy to grab the code, Download project to get a ready-to-build Cargo .zip, or Share to create a link you can send to others.",
  },
  {
    targetId: "grammar-panel",
    text: "Grammar reference shows the constructs this converter supports, plus links to format docs (Pest book or PEG on Wikipedia) and Marser docs. Check here first if a conversion fails.",
  },
];

let tourStepIndex = 0;
let tourHighlightEl = null;

function clearTourHighlight() {
  if (tourHighlightEl) {
    tourHighlightEl.classList.remove("tour-highlight");
    tourHighlightEl = null;
  }
}

function showTourStep(index) {
  const bar = document.getElementById("tour-bar");
  const text = document.getElementById("tour-text");
  const nextBtn = document.getElementById("tour-next");
  if (!bar || !text) return;

  clearTourHighlight();
  const step = TOUR_STEPS[index];
  if (!step) {
    finishTour();
    return;
  }

  const target = document.getElementById(step.targetId);
  if (target) {
    // Auto-open <details> elements so the content is visible during the tour step
    if (target.tagName === "DETAILS" && !target.open) {
      target.open = true;
    }
    target.classList.add("tour-highlight");
    tourHighlightEl = target;
    target.scrollIntoView({ block: "center", behavior: "smooth" });
  }

  text.textContent = `Step ${index + 1} of ${TOUR_STEPS.length}: ${step.text}`;
  if (nextBtn) {
    nextBtn.textContent = index === TOUR_STEPS.length - 1 ? "Done" : "Next";
  }
  bar.hidden = false;
}

function finishTour() {
  clearTourHighlight();
  const bar = document.getElementById("tour-bar");
  if (bar) bar.hidden = true;
  try {
    localStorage.setItem("grammar-to-marser.tour-done", "1");
  } catch {
    // ignore
  }
}

function startTour({ force = false } = {}) {
  if (!force) {
    try {
      if (localStorage.getItem("grammar-to-marser.tour-done") === "1") return;
    } catch {
      // ignore
    }
  }
  tourStepIndex = 0;
  showTourStep(tourStepIndex);
}

function initTour() {
  const bar = document.getElementById("tour-bar");
  const skip = document.getElementById("tour-skip");
  const next = document.getElementById("tour-next");
  if (!bar) return;

  skip?.addEventListener("click", finishTour);
  next?.addEventListener("click", () => {
    tourStepIndex += 1;
    if (tourStepIndex >= TOUR_STEPS.length) {
      finishTour();
    } else {
      showTourStep(tourStepIndex);
    }
  });

  // Tour button in the toolbar
  document.getElementById("tour-btn")?.addEventListener("click", () => {
    startTour({ force: true });
  });
}

export function initPaneResizer(onResize) {
  const resizer = document.getElementById("pane-resizer");
  const grammarPane = document.getElementById("grammar-pane");
  const rustPane = document.getElementById("rust-pane");
  const panes = document.getElementById("panes");
  if (!resizer || !grammarPane || !rustPane || !panes) return;

  const storageKey = "grammar-to-marser.split-ratio";
  const saved = parseFloat(localStorage.getItem(storageKey) || "0.5");
  if (!Number.isNaN(saved) && saved > 0.1 && saved < 0.9) {
    grammarPane.style.flex = `${saved} 1 0`;
    rustPane.style.flex = `${1 - saved} 1 0`;
  }

  let dragging = false;

  function onPointerMove(clientX) {
    const rect = panes.getBoundingClientRect();
    if (rect.width <= 0) return;
    const ratio = Math.min(0.85, Math.max(0.15, (clientX - rect.left) / rect.width));
    grammarPane.style.flex = `${ratio} 1 0`;
    rustPane.style.flex = `${1 - ratio} 1 0`;
    onResize();
  }

  resizer.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const rect = panes.getBoundingClientRect();
    if (rect.width <= 0) return;
    const grammarWidth = grammarPane.getBoundingClientRect().width;
    const currentRatio = grammarWidth / rect.width;
    const step = e.shiftKey ? 0.1 : 0.02;
    const delta = e.key === "ArrowRight" ? step : -step;
    const ratio = Math.min(0.85, Math.max(0.15, currentRatio + delta));
    grammarPane.style.flex = `${ratio} 1 0`;
    rustPane.style.flex = `${1 - ratio} 1 0`;
    onResize();
    try {
      localStorage.setItem(storageKey, String(ratio));
    } catch {
      // ignore
    }
  });

  resizer.addEventListener("mousedown", (e) => {
    dragging = true;
    resizer.classList.add("dragging");
    e.preventDefault();
  });

  window.addEventListener("mousemove", (e) => {
    if (!dragging) return;
    onPointerMove(e.clientX);
  });

  window.addEventListener("mouseup", () => {
    if (!dragging) return;
    dragging = false;
    resizer.classList.remove("dragging");
    const rect = panes.getBoundingClientRect();
    const grammarWidth = grammarPane.getBoundingClientRect().width;
    const ratio = grammarWidth / rect.width;
    try {
      localStorage.setItem(storageKey, String(ratio));
    } catch {
      // ignore
    }
    onResize();
  });
}

export function updateRuleDatalist(ruleNames) {
  const datalist = document.getElementById("rule-names");
  if (!datalist) return;
  datalist.innerHTML = "";
  for (const name of ruleNames) {
    const opt = document.createElement("option");
    opt.value = name;
    datalist.appendChild(opt);
  }
}

export function updateEntryRuleHint(entryRule, ruleNames) {
  const hint = document.getElementById("entry-rule-hint");
  if (!hint) return;
  const trimmed = entryRule.trim();
  if (!trimmed || ruleNames.length === 0) {
    hint.textContent = "";
    hint.hidden = true;
    return;
  }
  if (!ruleNames.includes(trimmed)) {
    hint.textContent = `Unknown rule: ${trimmed}`;
    hint.hidden = false;
  } else {
    hint.textContent = "";
    hint.hidden = true;
  }
}

/**
 * @param {Array} errors
 * @param {function|null} onErrorClick
 * @param {function|null} isJumpable - optional predicate returning true if an error can be jumped to
 */
export function renderErrors(errors, onErrorClick = null, isJumpable = null) {
  const list = document.getElementById("error-list");
  const copyBtn = document.getElementById("copy-errors-btn");
  if (!list) return;

  list.innerHTML = "";
  if (!errors || errors.length === 0) {
    list.hidden = true;
    if (copyBtn) copyBtn.disabled = true;
    return;
  }

  list.hidden = false;
  if (copyBtn) copyBtn.disabled = false;
  for (const [index, err] of (errors || []).entries()) {
    const li = document.createElement("li");
    li.textContent = typeof err === "string" ? err : err.message;

    const canJump =
      typeof onErrorClick === "function" &&
      (isJumpable == null || isJumpable(err));

    if (canJump) {
      li.dataset.jumpable = "1";
      li.tabIndex = 0;
      li.setAttribute("role", "button");
      li.title = "Jump to error location";
      li.addEventListener("click", () => onErrorClick(index, err));
      li.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onErrorClick(index, err);
        }
      });
    }
    list.appendChild(li);
  }
}

export function clearErrors() {
  renderErrors([]);
}

export function errorsAsText(errors) {
  return (errors || [])
    .map((err) => (typeof err === "string" ? err : err.message))
    .join("\n");
}

export function setGrammarFilename(name) {
  const el = document.getElementById("grammar-filename");
  if (!el) return;
  if (name) {
    el.textContent = name;
    el.hidden = false;
  } else {
    el.textContent = "";
    el.hidden = true;
  }
}

export function setExampleDescription(text) {
  const el = document.getElementById("example-description");
  if (!el) return;
  if (text) {
    el.textContent = text;
    el.hidden = false;
  } else {
    el.textContent = "";
    el.hidden = true;
  }
}

export function setStatus(text, color) {
  const statusEl = document.getElementById("status");
  if (!statusEl) return;
  statusEl.textContent = text;
  if (color) {
    statusEl.style.color = color;
  }
}

export function flashButton(button, label = "Copied!") {
  if (!button) return;
  const original = button.textContent;
  const wasDisabled = button.disabled;
  button.textContent = label;
  button.disabled = true;
  setTimeout(() => {
    button.textContent = original;
    button.disabled = wasDisabled;
  }, 1500);
}
