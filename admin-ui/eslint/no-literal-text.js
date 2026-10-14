// Local ESLint rule: text shown to users goes through the catalogs (`t(...)`),
// never written in place. Reports words in JSX text, in text attributes, in
// JSX expressions (also ternaries and templates), and in the arguments of the
// panel's message setters. Symbols, numbers and empty strings are fine.

const WORDS = /[A-Za-z]{2,}/;
const ATTRIBUTES = new Set(["aria-label", "title", "placeholder", "alt", "label", "description", "eyebrow"]);
const SETTERS = new Set(["setError", "setMessage", "setFailure", "setFormErrors", "prompt", "alert", "confirm"]);
const TEXT_KEYS = new Set(["text", "help"]);

function wordy(node) {
  if (!node) return false;
  switch (node.type) {
    case "Literal":
      return typeof node.value === "string" && WORDS.test(node.value);
    case "TemplateLiteral":
      return node.quasis.some((q) => WORDS.test(q.value.cooked ?? ""));
    case "ConditionalExpression":
      return wordy(node.consequent) || wordy(node.alternate);
    case "LogicalExpression":
      return wordy(node.left) || wordy(node.right);
    case "ArrayExpression":
      return node.elements.some(wordy);
    default:
      return false;
  }
}

/** @type {import("eslint").Rule.RuleModule} */
const rule = {
  meta: {
    type: "problem",
    docs: { description: "User-visible text must come from the i18n catalogs" },
    messages: { literal: "Put user-visible text in admin-ui/src/i18n and use t(): {{text}}" },
    schema: [],
  },
  create(context) {
    const report = (node, text) => context.report({ node, messageId: "literal", data: { text: String(text).slice(0, 40) } });
    return {
      JSXText(node) {
        if (WORDS.test(node.value)) report(node, node.value.trim());
      },
      JSXAttribute(node) {
        const name = typeof node.name.name === "string" ? node.name.name : "";
        if (!ATTRIBUTES.has(name) || !node.value) return;
        const value = node.value.type === "JSXExpressionContainer" ? node.value.expression : node.value;
        if (wordy(value)) report(node, name);
      },
      JSXExpressionContainer(node) {
        if (node.parent.type === "JSXAttribute") return;
        if (wordy(node.expression)) report(node, "expression");
      },
      CallExpression(node) {
        const callee = node.callee.type === "MemberExpression" ? node.callee.property : node.callee;
        if (callee.type === "Identifier" && SETTERS.has(callee.name) && node.arguments.some(wordy)) report(node, callee.name);
      },
      Property(node) {
        const key = node.key.type === "Identifier" ? node.key.name : node.key.value;
        if (TEXT_KEYS.has(key) && wordy(node.value)) report(node, key);
      },
    };
  },
};

export default { rules: { "no-literal-text": rule } };
