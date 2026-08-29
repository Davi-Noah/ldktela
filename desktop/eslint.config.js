import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';

export default tseslint.config(
  { ignores: ['dist', 'src-tauri', 'src/api/types/**'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }],
      // CLAUDE.md 7: `any` proibido; use `unknown` e refine.
      '@typescript-eslint/no-explicit-any': 'error',
      // CLAUDE.md 2.6: token de usuario nunca em storage do navegador.
      'no-restricted-globals': [
        'error',
        { name: 'localStorage', message: 'Proibido (CLAUDE.md 2.6): use o cofre do core Rust.' },
        { name: 'sessionStorage', message: 'Proibido (CLAUDE.md 2.6): use o cofre do core Rust.' },
      ],
    },
  },
);
