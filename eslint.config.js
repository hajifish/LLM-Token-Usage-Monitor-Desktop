import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import react from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';

// 由 .eslintrc.json 迁移而来（eslint 8 → 10，flat config）。
// 规则集合保持等价：eslint recommended + typescript-eslint recommended
// + react recommended + react-hooks recommended，自定义规则原样保留。
export default [
  { ignores: ['dist/', 'node_modules/', 'src-tauri/'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  react.configs.flat.recommended,
  reactHooks.configs.flat.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    settings: {
      react: { version: 'detect' },
    },
    rules: {
      'react/react-in-jsx-scope': 'off',
      'react/no-unescaped-entities': 'warn',
    },
  },
];
