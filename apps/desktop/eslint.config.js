import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";

export default tseslint.config(
  { ignores: ["dist", "dist-preview", "src-tauri", "src/ipc/generated", "node_modules"] },
  js.configs.recommended,
  ...tseslint.configs.strict,
  // The Tauri isolation app (D-047): plain browser script in its own frame.
  { files: ["isolation/**/*.js"], languageOptions: { globals: { ...globals.browser }, sourceType: "script" } },
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: { globals: { ...globals.browser } },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      // Model output and document text are rendered as text, never as HTML.
      "no-restricted-syntax": [
        "error",
        {
          selector: "JSXAttribute[name.name='dangerouslySetInnerHTML']",
          message: "Never render HTML. Model and document text must be shown as plain text.",
        },
        {
          selector: "MemberExpression[property.name=/^(innerHTML|outerHTML)$/]",
          message: "Never write HTML strings into the DOM.",
        },
        {
          selector: "CallExpression[callee.property.name='insertAdjacentHTML']",
          message: "Never write HTML strings into the DOM.",
        },
      ],
      "no-eval": "error",
      "no-implied-eval": "error",
      "no-new-func": "error",
      "no-console": "error",
    },
  },
);
