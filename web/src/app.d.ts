// Set by `define` in vite.config.ts. vitest.config.ts does not load that file, so under test the
// name is undeclared at runtime and has to be read through `typeof`.
declare const __KOMUN_TILE_URL__: string | undefined;
