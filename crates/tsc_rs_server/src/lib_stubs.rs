//! Minimal TypeScript standard library type stubs.
//!
//! These provide the most common built-in types so the LSP server can offer
//! hover info, completions, and go-to-definition for globals like `console`,
//! `Array`, `Promise`, `Map`, `Set`, `Math`, `JSON`, `document`, etc.
//!
//! The stubs are intentionally minimal -- just enough for useful IDE support.

/// Minimal lib.d.ts content covering the most frequently used built-ins.
pub const LIB_STUBS: &str = r#"
// --- Primitive interfaces ---

interface Array<T> {
    length: number;
    push(...items: T[]): number;
    pop(): T | undefined;
    shift(): T | undefined;
    unshift(...items: T[]): number;
    concat(...items: T[][]): T[];
    join(separator?: string): string;
    slice(start?: number, end?: number): T[];
    splice(start: number, deleteCount?: number, ...items: T[]): T[];
    indexOf(searchElement: T, fromIndex?: number): number;
    lastIndexOf(searchElement: T, fromIndex?: number): number;
    includes(searchElement: T, fromIndex?: number): boolean;
    find(predicate: (value: T, index: number, obj: T[]) => boolean): T | undefined;
    findIndex(predicate: (value: T, index: number, obj: T[]) => boolean): number;
    filter(predicate: (value: T, index: number, array: T[]) => boolean): T[];
    map<U>(callbackfn: (value: T, index: number, array: T[]) => U): U[];
    reduce<U>(callbackfn: (previousValue: U, currentValue: T, currentIndex: number, array: T[]) => U, initialValue: U): U;
    forEach(callbackfn: (value: T, index: number, array: T[]) => void): void;
    every(predicate: (value: T, index: number, array: T[]) => boolean): boolean;
    some(predicate: (value: T, index: number, array: T[]) => boolean): boolean;
    sort(compareFn?: (a: T, b: T) => number): T[];
    reverse(): T[];
    flat<D extends number = 1>(depth?: D): T[];
    flatMap<U>(callback: (value: T, index: number, array: T[]) => U[]): U[];
    fill(value: T, start?: number, end?: number): T[];
    entries(): IterableIterator<[number, T]>;
    keys(): IterableIterator<number>;
    values(): IterableIterator<T>;
}

interface String {
    length: number;
    charAt(pos: number): string;
    charCodeAt(index: number): number;
    concat(...strings: string[]): string;
    indexOf(searchString: string, position?: number): number;
    lastIndexOf(searchString: string, position?: number): number;
    includes(searchString: string, position?: number): boolean;
    startsWith(searchString: string, position?: number): boolean;
    endsWith(searchString: string, endPosition?: number): boolean;
    slice(start?: number, end?: number): string;
    substring(start: number, end?: number): string;
    toLowerCase(): string;
    toUpperCase(): string;
    trim(): string;
    trimStart(): string;
    trimEnd(): string;
    padStart(maxLength: number, fillString?: string): string;
    padEnd(maxLength: number, fillString?: string): string;
    repeat(count: number): string;
    replace(searchValue: string | RegExp, replaceValue: string): string;
    replaceAll(searchValue: string | RegExp, replaceValue: string): string;
    split(separator: string | RegExp, limit?: number): string[];
    match(regexp: string | RegExp): RegExpMatchArray | null;
    search(regexp: string | RegExp): number;
    at(index: number): string | undefined;
}

interface Number {
    toFixed(fractionDigits?: number): string;
    toExponential(fractionDigits?: number): string;
    toPrecision(precision?: number): string;
    toString(radix?: number): string;
    valueOf(): number;
}

interface Boolean {
    valueOf(): boolean;
}

interface Object {
    constructor: Function;
    toString(): string;
    valueOf(): Object;
    hasOwnProperty(v: string): boolean;
}

interface ObjectConstructor {
    assign<T extends object, U>(target: T, source: U): T & U;
    assign<T extends object, U, V>(target: T, source1: U, source2: V): T & U & V;
    assign<T extends object, U, V, W>(target: T, source1: U, source2: V, source3: W): T & U & V & W;
    assign(target: object, ...sources: any[]): any;
}

declare var Object: ObjectConstructor;

interface Function {
    apply(thisArg: any, argArray?: any): any;
    call(thisArg: any, ...argArray: any[]): any;
    bind(thisArg: any, ...argArray: any[]): Function;
    length: number;
    name: string;
}

interface RegExp {
    exec(string: string): RegExpExecArray | null;
    test(string: string): boolean;
    source: string;
    global: boolean;
    ignoreCase: boolean;
    multiline: boolean;
    flags: string;
    lastIndex: number;
}

interface RegExpMatchArray extends Array<string> {
    index: number;
    input: string;
}

interface RegExpExecArray extends Array<string> {
    index: number;
    input: string;
}

// --- Promise ---

interface Promise<T> {
    then<TResult1 = T, TResult2 = never>(
        onfulfilled?: (value: T) => TResult1 | PromiseLike<TResult1>,
        onrejected?: (reason: any) => TResult2 | PromiseLike<TResult2>
    ): Promise<TResult1 | TResult2>;
    catch<TResult = never>(onrejected?: (reason: any) => TResult | PromiseLike<TResult>): Promise<T | TResult>;
    finally(onfinally?: () => void): Promise<T>;
}

interface PromiseLike<T> {
    then<TResult1 = T, TResult2 = never>(
        onfulfilled?: (value: T) => TResult1 | PromiseLike<TResult1>,
        onrejected?: (reason: any) => TResult2 | PromiseLike<TResult2>
    ): PromiseLike<TResult1 | TResult2>;
}

interface PromiseConstructor {
    new <T>(executor: (resolve: (value: T) => void, reject: (reason?: any) => void) => void): Promise<T>;
    all<T>(values: Promise<T>[]): Promise<T[]>;
    allSettled<T>(values: Promise<T>[]): Promise<{ status: string; value?: T; reason?: any }[]>;
    any<T>(values: Promise<T>[]): Promise<T>;
    race<T>(values: Promise<T>[]): Promise<T>;
    resolve<T>(value: T): Promise<T>;
    reject(reason?: any): Promise<never>;
}

declare var Promise: PromiseConstructor;

// --- Map and Set ---

interface Map<K, V> {
    size: number;
    get(key: K): V | undefined;
    set(key: K, value: V): Map<K, V>;
    has(key: K): boolean;
    delete(key: K): boolean;
    clear(): void;
    forEach(callbackfn: (value: V, key: K, map: Map<K, V>) => void): void;
    keys(): IterableIterator<K>;
    values(): IterableIterator<V>;
    entries(): IterableIterator<[K, V]>;
}

interface MapConstructor {
    new <K, V>(entries?: [K, V][]): Map<K, V>;
}
declare var Map: MapConstructor;

interface Set<T> {
    size: number;
    add(value: T): Set<T>;
    has(value: T): boolean;
    delete(value: T): boolean;
    clear(): void;
    forEach(callbackfn: (value: T, value2: T, set: Set<T>) => void): void;
    keys(): IterableIterator<T>;
    values(): IterableIterator<T>;
    entries(): IterableIterator<[T, T]>;
}

interface SetConstructor {
    new <T>(values?: T[]): Set<T>;
}
declare var Set: SetConstructor;

interface WeakMap<K extends object, V> {
    get(key: K): V | undefined;
    set(key: K, value: V): WeakMap<K, V>;
    has(key: K): boolean;
    delete(key: K): boolean;
}

interface WeakSet<T extends object> {
    add(value: T): WeakSet<T>;
    has(value: T): boolean;
    delete(value: T): boolean;
}

// --- Iterators ---

interface IterableIterator<T> {
    next(): IteratorResult<T>;
}

interface IteratorResult<T> {
    done: boolean;
    value: T;
}

// --- Error types ---

interface Error {
    name: string;
    message: string;
    stack?: string;
}

interface ErrorConstructor {
    new (message?: string): Error;
}
declare var Error: ErrorConstructor;
declare var TypeError: ErrorConstructor;
declare var RangeError: ErrorConstructor;
declare var SyntaxError: ErrorConstructor;
declare var ReferenceError: ErrorConstructor;

// --- console ---

interface Console {
    log(...data: any[]): void;
    warn(...data: any[]): void;
    error(...data: any[]): void;
    info(...data: any[]): void;
    debug(...data: any[]): void;
    dir(item?: any): void;
    table(tabularData?: any): void;
    time(label?: string): void;
    timeEnd(label?: string): void;
    timeLog(label?: string, ...data: any[]): void;
    trace(...data: any[]): void;
    assert(condition?: boolean, ...data: any[]): void;
    clear(): void;
    count(label?: string): void;
    countReset(label?: string): void;
    group(...data: any[]): void;
    groupEnd(): void;
}

declare var console: Console;

// --- Math ---

interface Math {
    E: number;
    LN10: number;
    LN2: number;
    LOG2E: number;
    LOG10E: number;
    PI: number;
    SQRT1_2: number;
    SQRT2: number;
    abs(x: number): number;
    acos(x: number): number;
    asin(x: number): number;
    atan(x: number): number;
    atan2(y: number, x: number): number;
    ceil(x: number): number;
    cos(x: number): number;
    exp(x: number): number;
    floor(x: number): number;
    log(x: number): number;
    max(...values: number[]): number;
    min(...values: number[]): number;
    pow(x: number, y: number): number;
    random(): number;
    round(x: number): number;
    sin(x: number): number;
    sqrt(x: number): number;
    tan(x: number): number;
    trunc(x: number): number;
    sign(x: number): number;
    cbrt(x: number): number;
    hypot(...values: number[]): number;
    clz32(x: number): number;
    fround(x: number): number;
    imul(x: number, y: number): number;
}

declare var Math: Math;

// --- JSON ---

interface JSON {
    parse(text: string, reviver?: (key: string, value: any) => any): any;
    stringify(value: any, replacer?: (key: string, value: any) => any, space?: string | number): string;
    stringify(value: any, replacer?: (string | number)[], space?: string | number): string;
}

declare var JSON: JSON;

// --- Date ---

interface Date {
    getTime(): number;
    getFullYear(): number;
    getMonth(): number;
    getDate(): number;
    getDay(): number;
    getHours(): number;
    getMinutes(): number;
    getSeconds(): number;
    getMilliseconds(): number;
    setFullYear(year: number): number;
    setMonth(month: number): number;
    setDate(date: number): number;
    setHours(hours: number): number;
    setMinutes(min: number): number;
    setSeconds(sec: number): number;
    setMilliseconds(ms: number): number;
    toISOString(): string;
    toJSON(): string;
    toLocaleDateString(): string;
    toLocaleTimeString(): string;
    toLocaleString(): string;
    toString(): string;
    valueOf(): number;
}

interface DateConstructor {
    new (): Date;
    new (value: number): Date;
    new (value: string): Date;
    now(): number;
    parse(s: string): number;
}
declare var Date: DateConstructor;

// --- DOM (minimal) ---

interface Document {
    getElementById(elementId: string): HTMLElement | null;
    getElementsByClassName(classNames: string): HTMLCollectionOf<Element>;
    getElementsByTagName(qualifiedName: string): HTMLCollectionOf<Element>;
    querySelector(selectors: string): Element | null;
    querySelectorAll(selectors: string): NodeListOf<Element>;
    createElement(tagName: string): HTMLElement;
    createTextNode(data: string): Text;
    body: HTMLElement;
    head: HTMLElement;
    title: string;
    documentElement: HTMLElement;
    addEventListener(type: string, listener: (ev: Event) => any): void;
    removeEventListener(type: string, listener: (ev: Event) => any): void;
}

interface HTMLElement {
    id: string;
    className: string;
    innerHTML: string;
    innerText: string;
    textContent: string | null;
    style: CSSStyleDeclaration;
    parentElement: HTMLElement | null;
    children: HTMLCollectionOf<Element>;
    classList: DOMTokenList;
    getAttribute(qualifiedName: string): string | null;
    setAttribute(qualifiedName: string, value: string): void;
    removeAttribute(qualifiedName: string): void;
    addEventListener(type: string, listener: (ev: Event) => any): void;
    removeEventListener(type: string, listener: (ev: Event) => any): void;
    appendChild(node: Node): Node;
    removeChild(child: Node): Node;
    querySelector(selectors: string): Element | null;
    querySelectorAll(selectors: string): NodeListOf<Element>;
}

interface Element {
    tagName: string;
    id: string;
    className: string;
    innerHTML: string;
    getAttribute(qualifiedName: string): string | null;
    setAttribute(qualifiedName: string, value: string): void;
}

interface Node {
    parentNode: Node | null;
    childNodes: NodeListOf<Node>;
    firstChild: Node | null;
    lastChild: Node | null;
    nextSibling: Node | null;
    previousSibling: Node | null;
    textContent: string | null;
    appendChild(node: Node): Node;
    removeChild(child: Node): Node;
}

interface Text extends Node {
    data: string;
}

interface Event {
    type: string;
    target: EventTarget | null;
    preventDefault(): void;
    stopPropagation(): void;
}

interface EventTarget {
    addEventListener(type: string, listener: (ev: Event) => any): void;
    removeEventListener(type: string, listener: (ev: Event) => any): void;
}

interface HTMLCollectionOf<T> {
    length: number;
    item(index: number): T | null;
}

interface NodeListOf<T> {
    length: number;
    item(index: number): T | null;
    forEach(callbackfn: (value: T, key: number, parent: NodeListOf<T>) => void): void;
}

interface DOMTokenList {
    length: number;
    add(...tokens: string[]): void;
    remove(...tokens: string[]): void;
    contains(token: string): boolean;
    toggle(token: string, force?: boolean): boolean;
}

interface CSSStyleDeclaration {
    [index: string]: string;
}

declare var document: Document;
declare var window: Window;

interface Window {
    document: Document;
    location: Location;
    navigator: Navigator;
    console: Console;
    setTimeout(handler: Function, timeout?: number): number;
    clearTimeout(id: number): void;
    setInterval(handler: Function, timeout?: number): number;
    clearInterval(id: number): void;
    requestAnimationFrame(callback: (time: number) => void): number;
    cancelAnimationFrame(handle: number): void;
    fetch(input: string, init?: RequestInit): Promise<Response>;
    alert(message?: any): void;
    addEventListener(type: string, listener: (ev: Event) => any): void;
    removeEventListener(type: string, listener: (ev: Event) => any): void;
}

interface Location {
    href: string;
    hostname: string;
    pathname: string;
    protocol: string;
    search: string;
    hash: string;
    origin: string;
    reload(): void;
    assign(url: string): void;
    replace(url: string): void;
}

interface Navigator {
    userAgent: string;
    language: string;
    languages: string[];
    platform: string;
}

// --- Fetch API ---

interface Body {
    json(): Promise<any>;
    text(): Promise<string>;
    blob(): Promise<Blob>;
    arrayBuffer(): Promise<ArrayBuffer>;
    formData(): Promise<FormData>;
    body: ReadableStream<Uint8Array> | null;
    bodyUsed: boolean;
}

interface RequestInit {
    method?: string;
    headers?: HeadersInit;
    body?: BodyInit | null;
    mode?: string;
    credentials?: string;
    cache?: string;
    redirect?: string;
    referrer?: string;
    integrity?: string;
    signal?: AbortSignal | null;
}

type HeadersInit = Headers | Record<string, string> | [string, string][];
type BodyInit = string | Blob | ArrayBuffer | FormData | URLSearchParams | ReadableStream;

interface Request extends Body {
    method: string;
    url: string;
    headers: Headers;
    redirect: string;
    signal: AbortSignal;
    clone(): Request;
}

interface RequestConstructor {
    new (input: string | Request, init?: RequestInit): Request;
}
declare var Request: RequestConstructor;

interface Response extends Body {
    ok: boolean;
    status: number;
    statusText: string;
    headers: Headers;
    url: string;
    type: string;
    redirected: boolean;
    clone(): Response;
}

interface ResponseConstructor {
    new (body?: BodyInit | null, init?: ResponseInit): Response;
    json(data: any, init?: ResponseInit): Response;
    redirect(url: string, status?: number): Response;
    error(): Response;
}
declare var Response: ResponseConstructor;

interface ResponseInit {
    status?: number;
    statusText?: string;
    headers?: HeadersInit;
}

interface Headers {
    get(name: string): string | null;
    set(name: string, value: string): void;
    has(name: string): boolean;
    delete(name: string): void;
    append(name: string, value: string): void;
    entries(): IterableIterator<[string, string]>;
    keys(): IterableIterator<string>;
    values(): IterableIterator<string>;
    forEach(callbackfn: (value: string, key: string, parent: Headers) => void): void;
}

interface HeadersConstructor {
    new (init?: HeadersInit): Headers;
}
declare var Headers: HeadersConstructor;

interface URL {
    href: string;
    origin: string;
    protocol: string;
    hostname: string;
    port: string;
    pathname: string;
    search: string;
    hash: string;
    host: string;
    searchParams: URLSearchParams;
    toString(): string;
    toJSON(): string;
}

interface URLConstructor {
    new (url: string, base?: string): URL;
}
declare var URL: URLConstructor;

interface URLSearchParams {
    append(name: string, value: string): void;
    delete(name: string): void;
    get(name: string): string | null;
    getAll(name: string): string[];
    has(name: string): boolean;
    set(name: string, value: string): void;
    sort(): void;
    toString(): string;
    forEach(callbackfn: (value: string, key: string, parent: URLSearchParams) => void): void;
    entries(): IterableIterator<[string, string]>;
    keys(): IterableIterator<string>;
    values(): IterableIterator<string>;
}

interface URLSearchParamsConstructor {
    new (init?: string | Record<string, string> | [string, string][]): URLSearchParams;
}
declare var URLSearchParams: URLSearchParamsConstructor;

interface AbortSignal {
    aborted: boolean;
    reason: any;
    addEventListener(type: string, listener: (ev: Event) => any): void;
    removeEventListener(type: string, listener: (ev: Event) => any): void;
}

interface AbortController {
    signal: AbortSignal;
    abort(reason?: any): void;
}

interface AbortControllerConstructor {
    new (): AbortController;
}
declare var AbortController: AbortControllerConstructor;

interface FormData {
    append(name: string, value: string): void;
    delete(name: string): void;
    get(name: string): string | null;
    getAll(name: string): string[];
    has(name: string): boolean;
    set(name: string, value: string): void;
    entries(): IterableIterator<[string, string]>;
    keys(): IterableIterator<string>;
    values(): IterableIterator<string>;
}

interface ReadableStream<R = any> {
    locked: boolean;
    cancel(reason?: any): Promise<void>;
    getReader(): ReadableStreamDefaultReader<R>;
}

interface ReadableStreamDefaultReader<R = any> {
    read(): Promise<{ done: boolean; value: R }>;
    releaseLock(): void;
    cancel(reason?: any): Promise<void>;
}

interface Uint8Array {
    length: number;
    byteLength: number;
    byteOffset: number;
    slice(start?: number, end?: number): Uint8Array;
}

interface TextEncoder {
    encode(input?: string): Uint8Array;
    encoding: string;
}

interface TextEncoderConstructor {
    new (): TextEncoder;
}
declare var TextEncoder: TextEncoderConstructor;

interface TextDecoder {
    decode(input?: ArrayBuffer | Uint8Array): string;
    encoding: string;
}

interface TextDecoderConstructor {
    new (label?: string): TextDecoder;
}
declare var TextDecoder: TextDecoderConstructor;

interface Blob {
    size: number;
    type: string;
    slice(start?: number, end?: number, contentType?: string): Blob;
    text(): Promise<string>;
    arrayBuffer(): Promise<ArrayBuffer>;
    stream(): ReadableStream;
}

interface BlobConstructor {
    new (blobParts?: any[], options?: { type?: string }): Blob;
}
declare var Blob: BlobConstructor;

interface ArrayBuffer {
    byteLength: number;
    slice(begin: number, end?: number): ArrayBuffer;
}

interface ArrayBufferConstructor {
    new (byteLength: number): ArrayBuffer;
    isView(arg: any): boolean;
}
declare var ArrayBuffer: ArrayBufferConstructor;

// --- Timers and globals ---

declare function setTimeout(handler: Function, timeout?: number, ...args: any[]): number;
declare function clearTimeout(id: number): void;
declare function setInterval(handler: Function, timeout?: number, ...args: any[]): number;
declare function clearInterval(id: number): void;
declare function queueMicrotask(callback: () => void): void;
declare function structuredClone<T>(value: T): T;
declare function fetch(input: string | Request, init?: RequestInit): Promise<Response>;
declare function atob(data: string): string;
declare function btoa(data: string): string;
declare function parseInt(string: string, radix?: number): number;
declare function parseFloat(string: string): number;
declare function isNaN(number: number): boolean;
declare function isFinite(number: number): boolean;
declare function encodeURIComponent(uriComponent: string): string;
declare function decodeURIComponent(encodedURIComponent: string): string;
declare function encodeURI(uri: string): string;
declare function decodeURI(encodedURI: string): string;
declare function escape(string: string): string;
declare function unescape(string: string): string;

// --- process (Node.js) ---

interface Process {
    env: { [key: string]: string | undefined };
    argv: string[];
    cwd(): string;
    exit(code?: number): never;
    pid: number;
    platform: string;
    version: string;
    stdout: { write(data: string): boolean };
    stderr: { write(data: string): boolean };
}
declare var process: Process;

// --- Buffer (Node.js) ---

interface Buffer extends Uint8Array {
    toString(encoding?: string): string;
    toJSON(): { type: "Buffer"; data: number[] };
    write(string: string, encoding?: string): number;
    slice(start?: number, end?: number): Buffer;
    copy(target: Buffer, targetStart?: number, sourceStart?: number, sourceEnd?: number): number;
    compare(target: Buffer): number;
    equals(otherBuffer: Buffer): boolean;
    indexOf(value: string | number | Buffer): number;
    includes(value: string | number | Buffer): boolean;
}

interface BufferConstructor {
    from(data: string | ArrayBuffer | number[] | Buffer, encoding?: string): Buffer;
    alloc(size: number, fill?: number | string): Buffer;
    allocUnsafe(size: number): Buffer;
    concat(list: Buffer[], totalLength?: number): Buffer;
    isBuffer(obj: any): obj is Buffer;
    byteLength(string: string, encoding?: string): number;
}
declare var Buffer: BufferConstructor;

// --- Symbol (minimal) ---

interface SymbolConstructor {
    iterator: symbol;
    hasInstance: symbol;
    toPrimitive: symbol;
    toStringTag: symbol;
}
declare var Symbol: SymbolConstructor;

// --- Record/Partial/Required/Readonly (utility types) ---

type Partial<T> = { [P in keyof T]?: T[P] };
type Required<T> = { [P in keyof T]-?: T[P] };
type Readonly<T> = { readonly [P in keyof T]: T[P] };
type Record<K extends string | number | symbol, T> = { [P in K]: T };
type Pick<T, K extends keyof T> = { [P in K]: T[P] };
type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;
type NonNullable<T> = T extends null | undefined ? never : T;
type ReturnType<T extends (...args: any) => any> = T extends (...args: any) => infer R ? R : any;
type Parameters<T extends (...args: any) => any> = T extends (...args: infer P) => any ? P : never;
type InstanceType<T extends abstract new (...args: any) => any> = T extends abstract new (...args: any) => infer R ? R : any;
type Awaited<T> = T extends PromiseLike<infer U> ? Awaited<U> : T;
"#;
