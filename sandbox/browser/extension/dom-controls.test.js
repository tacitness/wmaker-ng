#!/usr/bin/env node
"use strict";

const assert = require("assert");
const controls = require("./dom-controls.js");

class FakeElement {
  constructor(tagName, attrs = {}, options = {}) {
    this.tagName = tagName.toUpperCase();
    this.attrs = attrs;
    this.children = [];
    this.parentElement = null;
    this.nodeType = 1;
    this.textContent = options.text || "";
    this.disabled = Boolean(options.disabled);
    this.hidden = Boolean(options.hidden);
    this.checked = options.checked;
    this.selected = options.selected;
    this.labels = options.labels || [];
    this.ownerDocument = options.ownerDocument || null;
    this.rect = options.rect || { left: 0, top: 0, right: 100, bottom: 20, width: 100, height: 20 };
  }

  append(child) {
    child.parentElement = this;
    child.ownerDocument = this.ownerDocument;
    this.children.push(child);
    return child;
  }

  getAttribute(name) {
    return Object.prototype.hasOwnProperty.call(this.attrs, name) ? this.attrs[name] : null;
  }

  getBoundingClientRect() {
    return this.rect;
  }

  matches(selector) {
    if (selector === "input[type='hidden'], [aria-hidden='true']") {
      return (
        this.tagName === "INPUT" && this.getAttribute("type") === "hidden"
      ) || this.getAttribute("aria-hidden") === "true";
    }
    return false;
  }
}

class FakeDocument {
  constructor(elements) {
    this.elements = elements;
    this.documentElement = { clientWidth: 1280, clientHeight: 720 };
    for (const element of elements) {
      element.ownerDocument = this;
    }
  }

  querySelectorAll() {
    return this.elements;
  }

  getElementById(id) {
    return this.elements.find((element) => element.getAttribute("id") === id) || null;
  }
}

globalThis.innerWidth = 1280;
globalThis.innerHeight = 720;
globalThis.getComputedStyle = () => ({ display: "block", visibility: "visible", opacity: "1" });

const root = new FakeElement("main", {}, { rect: { left: 0, top: 0, right: 1280, bottom: 720, width: 1280, height: 720 } });
const label = root.append(new FakeElement("span", { id: "submit-label" }, { text: "Send form" }));
const button = root.append(new FakeElement("button", { "aria-labelledby": "submit-label" }, {
  text: "Ignored fallback",
  rect: { left: 10.2, top: 20.6, right: 110.7, bottom: 60.1, width: 100.5, height: 39.5 }
}));
const link = root.append(new FakeElement("a", { href: "/docs" }, { text: "Read docs" }));
const passwordLabel = new FakeElement("label", {}, { text: "Password" });
const password = root.append(new FakeElement("input", { type: "password", name: "current-password" }, {
  labels: [passwordLabel],
  rect: { left: 50, top: 80, right: 250, bottom: 110, width: 200, height: 30 }
}));
const disabled = root.append(new FakeElement("input", { type: "text", placeholder: "Disabled field" }, {
  disabled: true
}));
const checkbox = root.append(new FakeElement("input", { type: "checkbox", "aria-label": "Agree" }, {
  checked: true
}));

const doc = new FakeDocument([root, label, button, link, password, disabled, checkbox]);

const found = controls.collectControls(doc);
const byLabel = Object.fromEntries(found.map((control) => [control.label, control]));

assert.equal(byLabel["Send form"].role, "button");
assert.deepEqual(byLabel["Send form"].bounds, { x: 10, y: 21, width: 101, height: 40 });
assert.match(byLabel["Send form"].handle, /^dom:/);

assert.equal(byLabel["Read docs"].role, "link");
assert.equal(byLabel.Password.role, "textbox");
assert.equal(byLabel.Password.redacted, true);
assert.equal(byLabel.Password.input_type, "password");
assert.equal(Object.prototype.hasOwnProperty.call(byLabel.Password, "value"), false);

assert.equal(byLabel["Disabled field"].enabled, false);
assert.equal(byLabel.Agree.role, "checkbox");
assert.equal(byLabel.Agree.checked, true);

console.log(`dom-controls.test.js ok (${found.length} controls)`);
