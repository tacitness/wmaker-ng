(function (root) {
  "use strict";

  const SELECTOR = [
    "a[href]",
    "button",
    "input",
    "select",
    "textarea",
    "summary",
    "[role]",
    "[contenteditable='']",
    "[contenteditable='true']",
    "[tabindex]"
  ].join(",");

  const SECRET_TYPES = new Set(["password"]);

  function collectControls(doc) {
    const documentRef = doc || root.document;
    if (!documentRef) {
      return [];
    }
    const viewport = viewportSize(root, documentRef);
    return Array.from(documentRef.querySelectorAll(SELECTOR))
      .filter((element) => isVisibleControl(element, viewport))
      .map((element, index) => controlFromElement(element, index, viewport))
      .filter(Boolean);
  }

  function controlFromElement(element, index, viewport) {
    const role = roleFor(element);
    const bounds = boundsFor(element, viewport);
    if (!role || !bounds) {
      return null;
    }
    const inputType = inputTypeFor(element);
    const redacted = SECRET_TYPES.has(inputType) || hasSecretName(element);
    return {
      handle: stableHandle(element, role, index),
      role,
      label: labelFor(element),
      enabled: !isDisabled(element),
      bounds,
      checked: checkedState(element),
      selected: selectedState(element),
      redacted,
      input_type: inputType || undefined
    };
  }

  function roleFor(element) {
    const explicit = attr(element, "role");
    if (explicit) {
      return explicit.toLowerCase();
    }
    const tag = element.tagName.toLowerCase();
    if (tag === "a") {
      return "link";
    }
    if (tag === "button" || tag === "summary") {
      return "button";
    }
    if (tag === "select") {
      return "combobox";
    }
    if (tag === "textarea") {
      return "textbox";
    }
    if (tag === "input") {
      const type = inputTypeFor(element);
      if (type === "checkbox") {
        return "checkbox";
      }
      if (type === "radio") {
        return "radio";
      }
      if (type === "range") {
        return "slider";
      }
      if (["button", "submit", "reset", "image"].includes(type)) {
        return "button";
      }
      return "textbox";
    }
    if (attr(element, "contenteditable")) {
      return "textbox";
    }
    return explicit || "control";
  }

  function labelFor(element) {
    return firstText([
      attr(element, "aria-label"),
      labelledByText(element),
      element.labels ? Array.from(element.labels).map((label) => text(label)).join(" ") : "",
      attr(element, "alt"),
      attr(element, "title"),
      attr(element, "placeholder"),
      attr(element, "value"),
      text(element)
    ]);
  }

  function labelledByText(element) {
    const ids = attr(element, "aria-labelledby");
    const doc = element.ownerDocument;
    if (!ids || !doc || !doc.getElementById) {
      return "";
    }
    return ids
      .split(/\s+/)
      .map((id) => doc.getElementById(id))
      .filter(Boolean)
      .map((label) => text(label))
      .join(" ");
  }

  function isVisibleControl(element, viewport) {
    if (isDisabledByHiddenAttribute(element)) {
      return false;
    }
    if (element.matches && element.matches("input[type='hidden'], [aria-hidden='true']")) {
      return false;
    }
    const style = root.getComputedStyle ? root.getComputedStyle(element) : null;
    if (style && (style.display === "none" || style.visibility === "hidden" || style.opacity === "0")) {
      return false;
    }
    const bounds = boundsFor(element, viewport);
    return Boolean(bounds && bounds.width > 0 && bounds.height > 0);
  }

  function boundsFor(element, viewport) {
    const rect = element.getBoundingClientRect ? element.getBoundingClientRect() : null;
    if (!rect || rect.width <= 0 || rect.height <= 0) {
      return null;
    }
    const left = clamp(rect.left, 0, viewport.width);
    const top = clamp(rect.top, 0, viewport.height);
    const right = clamp(rect.right, 0, viewport.width);
    const bottom = clamp(rect.bottom, 0, viewport.height);
    if (right <= left || bottom <= top) {
      return null;
    }
    return {
      x: Math.round(left),
      y: Math.round(top),
      width: Math.round(right - left),
      height: Math.round(bottom - top)
    };
  }

  function stableHandle(element, role, index) {
    const parts = [];
    let node = element;
    while (node && node.nodeType === 1) {
      const tag = node.tagName.toLowerCase();
      const id = attr(node, "id");
      if (id) {
        parts.unshift(`${tag}#${id}`);
        break;
      }
      const parent = node.parentElement;
      const siblings = parent ? Array.from(parent.children).filter((child) => child.tagName === node.tagName) : [];
      const nth = siblings.indexOf(node) + 1;
      parts.unshift(`${tag}:nth-of-type(${Math.max(nth, 1)})`);
      node = parent;
    }
    return `dom:${hash(`${role}|${parts.join(">")}|${index}`)}`;
  }

  function isDisabled(element) {
    return Boolean(element.disabled) || attr(element, "aria-disabled") === "true";
  }

  function checkedState(element) {
    if (typeof element.checked === "boolean") {
      return element.checked;
    }
    const value = attr(element, "aria-checked");
    return value ? value === "true" : undefined;
  }

  function selectedState(element) {
    if (typeof element.selected === "boolean") {
      return element.selected;
    }
    const value = attr(element, "aria-selected");
    return value ? value === "true" : undefined;
  }

  function inputTypeFor(element) {
    if (element.tagName.toLowerCase() !== "input") {
      return "";
    }
    return (attr(element, "type") || "text").toLowerCase();
  }

  function hasSecretName(element) {
    const name = `${attr(element, "name")} ${attr(element, "autocomplete")}`.toLowerCase();
    return /\b(current-password|new-password|one-time-code)\b/.test(name);
  }

  function isDisabledByHiddenAttribute(element) {
    return Boolean(element.hidden) || attr(element, "hidden") !== "";
  }

  function attr(element, name) {
    const value = element.getAttribute ? element.getAttribute(name) : null;
    return typeof value === "string" ? value.trim() : "";
  }

  function text(element) {
    return (element.textContent || "").replace(/\s+/g, " ").trim();
  }

  function firstText(values) {
    const value = values.find((candidate) => candidate && candidate.trim());
    return value ? value.trim() : "";
  }

  function viewportSize(globalRef, documentRef) {
    const element = documentRef.documentElement || {};
    return {
      width: globalRef.innerWidth || element.clientWidth || 0,
      height: globalRef.innerHeight || element.clientHeight || 0
    };
  }

  function clamp(value, min, max) {
    return Math.min(Math.max(value, min), max);
  }

  function hash(value) {
    let h = 2166136261;
    for (let i = 0; i < value.length; i += 1) {
      h ^= value.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return (h >>> 0).toString(36);
  }

  const api = {
    collectControls,
    controlFromElement,
    roleFor,
    labelFor,
    boundsFor,
    stableHandle
  };

  root.WmakerBrowserAdapter = Object.assign(root.WmakerBrowserAdapter || {}, api);
  if (typeof module !== "undefined") {
    module.exports = api;
  }
})(typeof globalThis !== "undefined" ? globalThis : this);
