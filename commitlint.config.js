// commitlint configuration for Ymir
// Used by wagoid/commitlint-github-action in CI (advisory, not branch-protection required).
// See .gitmessage for the full format spec.

module.exports = {
  rules: {
    // Enforce the allowed type set.
    'type-enum': [
      2,
      'always',
      ['feat', 'fix', 'docs', 'refactor', 'perf', 'test', 'build', 'ci', 'chore'],
    ],
    'type-empty': [2, 'never'],

    // Scope is required and must be one of the known crate short-names or
    // cross-cutting aliases.
    'scope-enum': [
      2,
      'always',
      [
        'core',
        'catalog',
        'system',
        'atmosphere',
        'surface',
        'climate',
        'biome',
        'detail',
        'render',
        'storage',
        'ymir',
        'workspace',
      ],
    ],
    'scope-empty': [2, 'never'],

    // Subject must start with a TASK-ID token (e.g. "INFRA-11 - ").
    // The regex matches: one or more uppercase letters, a hyphen, one or more
    // digits, a space-dash-space separator, then any non-empty text.
    'subject-pattern': [
      2,
      'always',
      /^[A-Z]+-\d+ - .+/.source,
    ],
    'subject-empty': [2, 'never'],

    // No period at end of subject line.
    'subject-full-stop': [2, 'never', '.'],

    // Body and footer are optional; no hard length limits beyond the subject.
    'header-max-length': [2, 'always', 120],
  },
};
