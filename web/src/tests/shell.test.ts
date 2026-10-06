import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const appHtml = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../app.html'), 'utf8');
const head = appHtml.slice(appHtml.indexOf('<head>'), appHtml.indexOf('</head>') + '</head>'.length);

describe('served document shell', () => {
	it('H1: the head names the product before any script runs', () => {
		expect(head).toContain('<title>Komun</title>');
	});

	it('H2: the head references no absolute or protocol-relative origin', () => {
		expect(head.startsWith('<head>')).toBe(true);
		expect(head).not.toMatch(/https?:\/\//);
		expect(head).not.toMatch(/(?:href|src)=["']\/\//);
	});
});
