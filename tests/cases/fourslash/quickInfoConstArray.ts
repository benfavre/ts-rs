/// <reference path="fourslash.ts" />

// @Filename: /tests/cases/fourslash/quickInfoConstArray.ts
//// const dom/*1*/ains = [
////   "cdn.builder.io",
////   "localhost",
////   "example.com",
//// ];
////
//// const fro/*2*/zen = [
////   "cdn.builder.io",
////   "localhost",
////   "example.com",
//// ] as const;

verify.quickInfoAt("1", "const domains: string[]");
verify.quickInfoAt("2", "const frozen: readonly string[]");
