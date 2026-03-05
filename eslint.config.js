import js from '@eslint/js'
import tseslint from 'typescript-eslint'

export default [
  {
    ignores: ['node_modules/**', 'dist/**', 'coverage/**'],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['src/**/*.ts', 'tests/**/*.ts'],
    rules: {
      'no-console': 'off',
    },
  },
]

