const vscode = require('vscode');
const fs = require('fs');
const path = require('path');
const CodeUtil = require('../code-util/CodeUtil');
const CodeGlobal = require('../code-global/CodeGlobal');
const CodeRegistry = require('./CodeRegistry');
const { KEYWORDS, GLOBALS, SHORTCUTS, KEYWORD_DOC } = require('./CodeKeywords');
const CodeIndex = require('./CodeIndex');

/** Global builtin class names (VM / CLI / SDK). */
const BUILTIN_GLOBAL_CLASSES = [
  'Array',
  'File',
  'Https',
  'Json',
  'Map',
  'MicroTask',
  'String',
  'Util',
  'Zip',
];

class CodeAssist {
  /**
   * @param {string} value
   */
  static escapeRegExp(value) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  }

  /**
   * Prefer nearest definition before cursor; fallback to earliest.
   * @param {number[]} offsets
   * @param {number} cursorOffset
   * @returns {number}
   */
  static pickBestOffset(offsets, cursorOffset) {
    if (!offsets.length) {
      return -1;
    }
    const before = offsets.filter((o) => o <= cursorOffset);
    if (before.length) {
      return Math.max.apply(null, before);
    }
    return Math.min.apply(null, offsets);
  }

  /**
   * @param {string} text
   * @param {RegExp} re
   * @param {string} name
   * @returns {number[]}
   */
  static collectNameOffsets(text, re, name) {
    /** @type {number[]} */
    const out = [];
    let m;
    while ((m = re.exec(text)) !== null) {
      const idxInMatch = m[0].indexOf(name);
      if (idxInMatch >= 0) {
        out.push(m.index + idxInMatch);
      }
    }
    return out;
  }

  /**
   * @param {vscode.TextDocument} document
   * @param {number} startOffset
   * @param {number} length
   * @returns {vscode.Location}
   */
  static locationFromOffset(document, startOffset, length) {
    const start = document.positionAt(startOffset);
    const end = document.positionAt(startOffset + length);
    return new vscode.Location(document.uri, new vscode.Range(start, end));
  }

  /**
   * Resolve `require("...")` target under current document directory.
   * Supports exact path or implicit `.boyia` suffix.
   * @param {vscode.TextDocument} document
   * @param {string} requiredPath
   * @returns {vscode.Location | null}
   */
  static resolveRequireTargetLocation(document, requiredPath) {
    const resolvedPath = CodeAssist.resolveRequireTargetPath(document, requiredPath);
    if (!resolvedPath) {
      return null;
    }
    const uri = vscode.Uri.file(resolvedPath);
    return new vscode.Location(uri, new vscode.Range(new vscode.Position(0, 0), new vscode.Position(0, 0)));
  }

  /**
   * Resolve `require("...")` target file path under current document directory.
   * Supports exact path or implicit `.boyia` suffix.
   * @param {vscode.TextDocument} document
   * @param {string} requiredPath
   * @returns {string | null}
   */
  static resolveRequireTargetPath(document, requiredPath) {
    if (!requiredPath) {
      return null;
    }
    const baseFile = document.uri && document.uri.fsPath ? document.uri.fsPath : '';
    const baseDir = baseFile ? path.dirname(baseFile) : '';
    const resolved = path.isAbsolute(requiredPath)
      ? path.normalize(requiredPath)
      : path.resolve(baseDir || '.', requiredPath);
    const candidates = [resolved];
    if (!resolved.toLowerCase().endsWith('.boyia')) {
      candidates.push(`${resolved}.boyia`);
    }
    for (const filePath of candidates) {
      try {
        if (fs.existsSync(filePath) && fs.statSync(filePath).isFile()) {
          return filePath;
        }
      } catch (_e) {
        // Ignore inaccessible or malformed candidate and continue trying.
      }
    }
    return null;
  }

  /**
   * Resolve `require("...")` target URI under current document directory.
   * Supports exact path or implicit `.boyia` suffix.
   * @param {vscode.TextDocument} document
   * @param {string} requiredPath
   * @returns {vscode.Uri | null}
   */
  static resolveRequireTargetUri(document, requiredPath) {
    const resolvedPath = CodeAssist.resolveRequireTargetPath(document, requiredPath);
    return resolvedPath ? vscode.Uri.file(resolvedPath) : null;
  }

  /**
   * Parse all `require("...")` / `require('...')` calls in a document.
   * @param {string} text
   * @returns {{ path: string, literalStart: number, literalEnd: number }[]}
   */
  static parseRequireCalls(text) {
    const out = [];
    const re = /\brequire\s*\(\s*("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')\s*\)/g;
    let m;
    while ((m = re.exec(text)) !== null) {
      const quoted = m[1] || '';
      if (quoted.length < 2) {
        continue;
      }
      const quote = quoted[0];
      if ((quote !== '"' && quote !== '\'') || quoted[quoted.length - 1] !== quote) {
        continue;
      }
      const requiredPath = quoted.slice(1, -1);
      if (!requiredPath) {
        continue;
      }
      const idx = m[0].indexOf(quoted);
      if (idx < 0) {
        continue;
      }
      const literalStart = m.index + idx;
      const literalEnd = literalStart + quoted.length;
      out.push({ path: requiredPath, literalStart, literalEnd });
    }
    return out;
  }

  /**
   * @param {vscode.TextDocument} document
   * @returns {string[]}
   */
  static requiredFilePaths(document) {
    const text = document.getText();
    const requires = CodeAssist.parseRequireCalls(text);
    const out = [];
    const seen = new Set();
    for (const req of requires) {
      const filePath = CodeAssist.resolveRequireTargetPath(document, req.path);
      if (filePath && !seen.has(filePath)) {
        seen.add(filePath);
        out.push(filePath);
      }
    }
    return out;
  }

  /**
   * @param {string} text
   * @param {number} offset
   * @returns {vscode.Position}
   */
  static positionFromTextOffset(text, offset) {
    const o = Math.max(0, Math.min(offset, text.length));
    let line = 0;
    let character = 0;
    for (let i = 0; i < o; i++) {
      if (text.charCodeAt(i) === 10) {
        line++;
        character = 0;
      } else {
        character++;
      }
    }
    return new vscode.Position(line, character);
  }

  /**
   * @param {string} filePath
   * @param {number} startOffset
   * @param {number} length
   * @param {string} text
   * @returns {vscode.Location}
   */
  static locationInFile(filePath, startOffset, length, text) {
    const start = CodeAssist.positionFromTextOffset(text, startOffset);
    const end = CodeAssist.positionFromTextOffset(text, startOffset + length);
    return new vscode.Location(vscode.Uri.file(filePath), new vscode.Range(start, end));
  }

  /**
   * @param {string} filePath
   * @returns {string | null}
   */
  static readFileText(filePath) {
    try {
      const raw = fs.readFileSync(filePath, 'UTF-8');
      return raw.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
    } catch (_e) {
      return null;
    }
  }

  /**
   * @param {string} filePath
   * @param {string} className
   * @returns {vscode.Location | null}
   */
  static findClassLocationInFile(filePath, className) {
    const text = CodeAssist.readFileText(filePath);
    if (!text) {
      return null;
    }
    const safe = CodeAssist.escapeRegExp(className);
    const re = new RegExp(`\\bclass\\s+(${safe})\\b`, 'g');
    const m = re.exec(text);
    if (!m) {
      return null;
    }
    const idx = m[0].indexOf(className);
    if (idx < 0) {
      return null;
    }
    const startOffset = m.index + idx;
    return CodeAssist.locationInFile(filePath, startOffset, className.length, text);
  }

  /**
   * @param {string} filePath
   * @param {string} className
   * @param {string} memberName
   * @returns {vscode.Location | null}
   */
  static findMemberLocationInFile(filePath, className, memberName) {
    const text = CodeAssist.readFileText(filePath);
    if (!text) {
      return null;
    }
    const { classes } = CodeIndex.parseDocument(text);
    const memberOffset = CodeAssist.findMemberOffsetInHierarchy(text, className, memberName, classes);
    if (memberOffset < 0) {
      return null;
    }
    return CodeAssist.locationInFile(filePath, memberOffset, memberName.length, text);
  }

  /**
   * @param {vscode.TextDocument} document
   * @param {string} className
   * @returns {vscode.Location[]}
   */
  static importedClassLocations(document, className) {
    const files = CodeAssist.requiredFilePaths(document);
    for (const filePath of files) {
      const loc = CodeAssist.findClassLocationInFile(filePath, className);
      if (loc) {
        return [loc];
      }
    }
    return [];
  }

  /**
   * @param {vscode.TextDocument} document
   * @param {string} className
   * @param {string} memberName
   * @returns {vscode.Location[]}
   */
  static importedMemberLocations(document, className, memberName) {
    const files = CodeAssist.requiredFilePaths(document);
    for (const filePath of files) {
      const loc = CodeAssist.findMemberLocationInFile(filePath, className, memberName);
      if (loc) {
        return [loc];
      }
    }
    return [];
  }

  /**
   * If cursor is within `require("...")` string literal, return target location.
   * @param {vscode.TextDocument} document
   * @param {vscode.Position} position
   * @returns {vscode.Location | null}
   */
  static requireDefinitionLocation(document, position) {
    const text = document.getText();
    const offset = document.offsetAt(position);
    const requires = CodeAssist.parseRequireCalls(text);
    for (const req of requires) {
      // Support cursor on quote or inside the path content.
      const start = req.literalStart;
      const end = req.literalEnd;
      if (offset >= start && offset <= end) {
        return CodeAssist.resolveRequireTargetLocation(document, req.path);
      }
    }
    return null;
  }

  /**
   * Provide clickable links so whole require-string gets underlined.
   * @param {vscode.TextDocument} document
   * @returns {vscode.DocumentLink[]}
   */
  static provideDocumentLinks(document) {
    const text = document.getText();
    const requires = CodeAssist.parseRequireCalls(text);
    return requires
      .map(function (req) {
        const uri = CodeAssist.resolveRequireTargetUri(document, req.path);
        if (!uri) {
          return null;
        }
        const start = document.positionAt(req.literalStart);
        const end = document.positionAt(req.literalEnd);
        return new vscode.DocumentLink(new vscode.Range(start, end), uri);
      })
      .filter(function (l) { return l != null; });
  }

  /**
   * @param {string} text
   * @param {string} className
   * @returns {{ open: number, close: number } | null}
   */
  static findClassBlock(text, className) {
    const safe = CodeAssist.escapeRegExp(className);
    const re = new RegExp(`\\bclass\\s+${safe}(?:\\s+extends\\s+\\w+)?\\s*\\{`, 'g');
    const m = re.exec(text);
    if (!m) {
      return null;
    }
    const open = m.index + m[0].length - 1;
    const close = CodeIndex.indexOfMatchingBrace(text, open);
    if (close < 0) {
      return null;
    }
    return { open, close };
  }

  /**
   * @param {string} text
   * @param {string} className
   * @param {string} memberName
   * @returns {number}
   */
  static findMemberOffsetInClass(text, className, memberName) {
    const block = CodeAssist.findClassBlock(text, className);
    if (!block) {
      return -1;
    }
    const bodyStart = block.open + 1;
    const bodyEnd = block.close;
    const body = text.slice(bodyStart, bodyEnd);
    const safe = CodeAssist.escapeRegExp(memberName);
    const patterns = [
      new RegExp(`\\bprop\\s+async\\s+fun\\s+(${safe})\\s*\\(`, 'g'),
      new RegExp(`\\bprop\\s+fun\\s+(${safe})\\s*\\(`, 'g'),
      new RegExp(`\\bfun\\s+(${safe})\\s*\\(`, 'g'),
      new RegExp(`\\bprop\\s+(${safe})\\s*[=;]`, 'g'),
    ];
    for (const re of patterns) {
      const m = re.exec(body);
      if (m) {
        const idxInMatch = m[0].indexOf(memberName);
        if (idxInMatch >= 0) {
          return bodyStart + m.index + idxInMatch;
        }
      }
    }
    return -1;
  }

  /**
   * @param {string} text
   * @param {string} className
   * @param {string} memberName
   * @param {Map<string, { members: Set<string>, extends: string | null }>} classes
   * @param {Set<string>} visited
   * @returns {number}
   */
  static findMemberOffsetInHierarchy(text, className, memberName, classes, visited = new Set()) {
    if (!className || visited.has(className)) {
      return -1;
    }
    visited.add(className);
    const here = CodeAssist.findMemberOffsetInClass(text, className, memberName);
    if (here >= 0) {
      return here;
    }
    const info = classes.get(className);
    if (!info || !info.extends) {
      return -1;
    }
    return CodeAssist.findMemberOffsetInHierarchy(text, info.extends, memberName, classes, visited);
  }

  /**
   * @param {vscode.TextDocument} document
   * @param {vscode.Position} position
   * @param {string} className
   * @returns {vscode.Location[]}
   */
  static classDefinitionLocations(document, position, className) {
    const text = document.getText();
    const safe = CodeAssist.escapeRegExp(className);
    const cursorOffset = document.offsetAt(position);
    const re = new RegExp(`\\bclass\\s+(${safe})\\b`, 'g');
    const offsets = CodeAssist.collectNameOffsets(text, re, className);
    const picked = CodeAssist.pickBestOffset(offsets, cursorOffset);
    if (picked >= 0) {
      return [CodeAssist.locationFromOffset(document, picked, className.length)];
    }
    return CodeAssist.importedClassLocations(document, className);
  }

  /**
   * When the word regex treats `Receiver.member` as one token, split by cursor side of `.`.
   * @param {vscode.TextDocument} document
   * @param {vscode.Position} position
   * @returns {{ receiver: string, member: string, onReceiver: boolean, onMember: boolean } | null}
   */
  static dottedTokenAtCursor(document, position) {
    const wordRange = document.getWordRangeAtPosition(position, /[A-Za-z_][\w.]*/);
    if (!wordRange) {
      return null;
    }
    const token = document.getText(wordRange);
    const m = token.match(/^([A-Za-z_]\w*)\.(\w+)$/);
    if (!m) {
      return null;
    }
    const cursorOffset = document.offsetAt(position);
    const startOffset = document.offsetAt(wordRange.start);
    const dotOffset = startOffset + token.indexOf('.');
    return {
      receiver: m[1],
      member: m[2],
      onReceiver: cursorOffset < dotOffset,
      onMember: cursorOffset > dotOffset,
    };
  }

  /**
   * @param {vscode.TextDocument} document
   * @param {vscode.Position} position
   * @param {string} name
   * @returns {vscode.Location[]}
   */
  static symbolDefinitionLocations(document, position, name) {
    const text = document.getText();
    const safe = CodeAssist.escapeRegExp(name);
    const cursorOffset = document.offsetAt(position);
    const patterns = [
      new RegExp(`\\bprop\\s+async\\s+fun\\s+(${safe})\\s*\\(`, 'g'),
      new RegExp(`\\bprop\\s+fun\\s+(${safe})\\s*\\(`, 'g'),
      new RegExp(`\\bfun\\s+(${safe})\\s*\\(`, 'g'),
      new RegExp(`\\bprop\\s+(${safe})\\s*[=;]`, 'g'),
      new RegExp(`\\bvar\\s+(${safe})\\b`, 'g'),
    ];
    /** @type {number[]} */
    let offsets = [];
    for (const re of patterns) {
      offsets = offsets.concat(CodeAssist.collectNameOffsets(text, re, name));
    }
    offsets = Array.from(new Set(offsets));
    const picked = CodeAssist.pickBestOffset(offsets, cursorOffset);
    if (picked < 0) {
      return [];
    }
    return [CodeAssist.locationFromOffset(document, picked, name.length)];
  }

  /**
   * @param {vscode.TextDocument} document
   * @param {vscode.Position} position
   * @param {string} receiver
   * @param {string} memberName
   * @returns {vscode.Location[]}
   */
  static memberDefinitionLocations(document, position, receiver, memberName) {
    const text = document.getText();
    const offset = document.offsetAt(position);
    const { classes } = CodeIndex.parseDocument(text);

    /** @type {string | null} */
    let cls = null;
    if (receiver === 'this') {
      const c = CodeIndex.innermostClassAtOffset(text, offset);
      cls = c && c.name;
    } else {
      const map = CodeIndex.varToClassBeforeOffset(text, offset);
      cls = map.get(receiver) || null;
    }
    if (!cls) {
      if (receiver !== 'this') {
        cls = receiver;
      } else {
        return [];
      }
    }

    const memberOffset = CodeAssist.findMemberOffsetInHierarchy(text, cls, memberName, classes);
    if (memberOffset < 0) {
      return CodeAssist.importedMemberLocations(document, cls, memberName);
    }
    return [CodeAssist.locationFromOffset(document, memberOffset, memberName.length)];
  }

  /**
   * @param {vscode.CompletionItem} item
   * @returns {string}
   */
  static completionItemLabelString(item) {
    if (!item || item.label == null) {
      return '';
    }
    const lab = item.label;
    return typeof lab === 'string' ? lab : /** @type {{ label?: string }} */ (lab).label || '';
  }

  static async initialize() {
    try {
      await CodeAssist.linkBoyiaFile();
      const content = fs.readFileSync(CodeUtil.getAbsolutePath(CodeGlobal.context, 'config/assist.json'), 'UTF-8');
      CodeAssist.config = JSON.parse(content);
      if (!CodeAssist.config.namespaces) {
        CodeAssist.config.namespaces = {};
      }
      CodeRegistry.register();
    } catch (e) {
      console.error('CodeAssist::initialize', e);
      CodeAssist.config = { namespaces: {}, apiDocs: {} };
      CodeRegistry.register();
    }
  }

  /**
   * @param {vscode.CompletionItem[]} items
   */
  static dedupeCompletionItems(items) {
    const map = new Map();
    for (const it of items) {
      const lab = CodeAssist.completionItemLabelString(it);
      if (lab && !map.has(lab)) {
        map.set(lab, it);
      }
    }
    return Array.from(map.values());
  }

  /**
   * Text from line start to cursor (exclusive of rest of line).
   * @param {string} lineText
   * @param {number} character
   */
  static linePrefix(lineText, character) {
    const end = Math.min(character, lineText.length);
    return lineText.substring(0, end);
  }

  /**
   * `Class.` + optional partial method: last identifier segment with trailing dot.
   * @param {string} before
   * @returns {{ namespace: string, partial: string } | null}
   */
  static parseDottedCompletion(before) {
    const m = before.match(/(?:^|[^\w.])([A-Za-z_]\w*\.)([\w]*)$/);
    if (!m) {
      return null;
    }
    return { namespace: m[1], partial: m[2] || '' };
  }

  /**
   * Identifier or prefix being typed at end of `before` (no leading dot segment for Util.foo).
   * @param {string} before
   */
  static parseWordPrefix(before) {
    const m = before.match(/([A-Za-z_][\w]*)$/);
    return m ? m[1] : '';
  }

  /**
   * @param {string[]} names
   * @param {string} partial
   * @param {vscode.CompletionItemKind} kind
   * @param {Record<string, string>} [detailByName]
   * @param {string} [categoryDetail] default `detail` when no per-name entry
   */
  static filterCompletionNames(names, partial, kind, detailByName, categoryDetail) {
    const p = partial.toLowerCase();
    return names
      .filter((n) => !p || n.toLowerCase().startsWith(p))
      .map((n) => {
        const item = new vscode.CompletionItem(n, kind);
        item.insertText = n;
        if (detailByName && detailByName[n]) {
          item.detail = detailByName[n];
        } else if (categoryDetail) {
          item.detail = categoryDetail;
        }
        return item;
      });
  }

  /**
   * Merges legacy `config.util` into `namespaces['Util.']` if present.
   * @returns {Record<string, string[]>}
   */
  static getEffectiveNamespaces() {
    const cfg = CodeAssist.config || {};
    const raw = Object.assign({}, cfg.namespaces || {});
    if (Array.isArray(cfg.util) && cfg.util.length) {
      const u = new Set((raw['Util.'] || []).concat(cfg.util));
      raw['Util.'] = Array.from(u).sort(function (a, b) {
        return a.localeCompare(b);
      });
    }
    return raw;
  }

  /**
   * Built-in `Class.method` completions with optional `apiDocs` from assist.json.
   * @param {string[]} names
   * @param {string} partial
   * @param {string} namespaceKey e.g. `Util.`
   * @param {Record<string, unknown>} [config]
   */
  static filterBuiltinNamespaceMethods(names, partial, namespaceKey, config) {
    const cfg = config || CodeAssist.config || {};
    const apiDocs = cfg.apiDocs || {};
    const cls = namespaceKey.replace(/\.$/, '');
    const p = partial.toLowerCase();
    return names
      .filter((n) => !p || n.toLowerCase().startsWith(p))
      .map((n) => {
        const item = new vscode.CompletionItem(n, vscode.CompletionItemKind.Method);
        item.insertText = n;
        item.detail = `${cls} · 内置`;
        const docKey = `${cls}.${n}`;
        if (apiDocs[docKey]) {
          item.documentation = new vscode.MarkdownString(String(apiDocs[docKey]));
        }
        return item;
      });
  }

  /**
   * @param {string} partial
   */
  static builtinClassCompletionItems(partial) {
    const p = partial.toLowerCase();
    return BUILTIN_GLOBAL_CLASSES.filter((c) => !p || c.toLowerCase().startsWith(p)).map((c) => {
      const item = new vscode.CompletionItem(c, vscode.CompletionItemKind.Class);
      item.insertText = c;
      item.detail = '内置类';
      return item;
    });
  }

  /**
   * @param {string[]} words
   * @param {string} partial
   */
  static keywordItems(words, partial) {
    return CodeAssist.filterCompletionNames(words, partial, vscode.CompletionItemKind.Keyword, undefined, '关键字');
  }

  /**
   * Document-local symbols (var / params / fun / class / prop names) before cursor.
   * @param {vscode.TextDocument} document
   * @param {vscode.Position} position
   * @param {string} word prefix being typed
   * @returns {vscode.CompletionItem[]}
   */
  static documentSymbolCompletionItems(document, position, word) {
    if (!word) {
      return [];
    }
    const text = document.getText();
    const offset = document.offsetAt(position);
    const raw = CodeIndex.collectSymbolNames(text, offset);
    const p = word.toLowerCase();
    const builtinLc = new Set(BUILTIN_GLOBAL_CLASSES.map((c) => c.toLowerCase()));
    const kwLc = new Set(KEYWORDS.concat(GLOBALS).map((k) => k.toLowerCase()));
    const names = raw.filter(function (n) {
      if (!n.toLowerCase().startsWith(p)) {
        return false;
      }
      const nl = n.toLowerCase();
      if (builtinLc.has(nl) || kwLc.has(nl)) {
        return false;
      }
      return true;
    });
    return names.map(function (n) {
      const item = new vscode.CompletionItem(n, vscode.CompletionItemKind.Variable);
      item.insertText = n;
      item.detail = '变量';
      return item;
    });
  }

  /**
   * @param {vscode.TextDocument} document
   * @param {vscode.Position} position
   * @param {vscode.CancellationToken} _token
   * @param {vscode.CompletionContext} context
   * @returns {vscode.CompletionItem[] | undefined}
   */
  static provideCompletionItems(document, position, _token, context) {
    if (!CodeAssist.config) {
      return undefined;
    }

    const lineText = document.lineAt(position).text;
    const before = CodeAssist.linePrefix(lineText, position.character);

    const dotted = CodeAssist.parseDottedCompletion(before);
    if (dotted) {
      const { namespace, partial } = dotted;
      const nsMap = CodeAssist.getEffectiveNamespaces();
      if (Object.prototype.hasOwnProperty.call(nsMap, namespace) && Array.isArray(nsMap[namespace])) {
        return CodeAssist.filterBuiltinNamespaceMethods(nsMap[namespace], partial, namespace, CodeAssist.config);
      }

      const receiver = namespace.slice(0, -1);
      const fullText = document.getText();
      const offset = document.offsetAt(position);
      const { classes } = CodeIndex.parseDocument(fullText);

      /** @type {string | null} */
      let instClass = null;
      if (receiver === 'this') {
        const ic = CodeIndex.innermostClassAtOffset(fullText, offset);
        instClass = ic && ic.name;
      } else {
        const vmap = CodeIndex.varToClassBeforeOffset(fullText, offset);
        instClass = vmap.get(receiver) || null;
      }

      if (instClass && classes.has(instClass)) {
        const mems = CodeIndex.membersForClass(instClass, classes);
        /** @type {Record<string, string>} */
        const detail = {};
        for (const n of mems) {
          detail[n] = `成员 · ${instClass}`;
        }
        return CodeAssist.filterCompletionNames(
          mems,
          partial,
          vscode.CompletionItemKind.Method,
          detail
        );
      }
      if (instClass) {
        const nsKey = instClass + '.';
        if (Object.prototype.hasOwnProperty.call(nsMap, nsKey) && Array.isArray(nsMap[nsKey])) {
          return CodeAssist.filterBuiltinNamespaceMethods(nsMap[nsKey], partial, nsKey, CodeAssist.config);
        }
      }

      return undefined;
    }

    const word = CodeAssist.parseWordPrefix(before);
    if (!word) {
      if (context.triggerKind === vscode.CompletionTriggerKind.Invoke) {
        return CodeAssist.dedupeCompletionItems(
          CodeAssist.builtinClassCompletionItems('').concat(
            CodeAssist.keywordItems(KEYWORDS.concat(GLOBALS), '')
          )
        );
      }
      return undefined;
    }

    /** @type {vscode.CompletionItem[]} */
    let items = [];

    items = items.concat(CodeAssist.builtinClassCompletionItems(word));

    if (CodeRegistry.registers && CodeRegistry.registers[word]) {
      const fromRegistry = CodeRegistry.registers[word].exec(word, CodeAssist.config);
      if (fromRegistry && fromRegistry.length) {
        items = items.concat(fromRegistry);
      }
    }

    const lastChar = word[word.length - 1];
    if (word.length === 1 && SHORTCUTS[lastChar]) {
      items = items.concat(CodeAssist.keywordItems(SHORTCUTS[lastChar], ''));
    }

    const merged = new Set(items.map((i) => CodeAssist.completionItemLabelString(i)));
    const kw = KEYWORDS.concat(GLOBALS).filter(function (k) {
      return k.toLowerCase().startsWith(word.toLowerCase()) && !merged.has(k);
    });
    items = items.concat(CodeAssist.keywordItems(kw, ''));

    items = items.concat(CodeAssist.documentSymbolCompletionItems(document, position, word));

    const out = CodeAssist.dedupeCompletionItems(items);
    return out.length ? out : undefined;
  }

  static resolveCompletionItem(item, _token) {
    const label = CodeAssist.completionItemLabelString(item);
    if (KEYWORD_DOC[label]) {
      item.documentation = new vscode.MarkdownString(KEYWORD_DOC[label]);
    }
    return item;
  }

  /**
   * @param {vscode.TextDocument} document
   * @param {vscode.Position} position
   */
  static provideHover(document, position) {
    if (!CodeAssist.config) {
      return null;
    }
    const range = document.getWordRangeAtPosition(position, /[A-Za-z_][\w.]*/);
    if (!range) {
      return null;
    }
    const word = document.getText(range);
    const doc = KEYWORD_DOC[word];
    if (doc) {
      const md = new vscode.MarkdownString(doc);
      md.isTrusted = true;
      return new vscode.Hover(md, range);
    }

    const dotted = word.match(/^([A-Za-z_]\w*)\.(\w+)$/);
    if (dotted) {
      const ns = `${dotted[1]}.`;
      const method = dotted[2];
      const nsMap = CodeAssist.getEffectiveNamespaces();
      const apis = nsMap[ns];
      if (Array.isArray(apis) && apis.includes(method)) {
        const cls = dotted[1];
        const docKey = `${cls}.${method}`;
        const apiDoc = (CodeAssist.config.apiDocs && CodeAssist.config.apiDocs[docKey]) || '';
        const body = apiDoc ? `${apiDoc}\n\n` : '';
        const md = new vscode.MarkdownString(`${body}\`${word}\` — **${cls}** 内置方法（\`assist.json\`）。`);
        md.isTrusted = true;
        return new vscode.Hover(md, range);
      }

      const fullText = document.getText();
      const offset = document.offsetAt(range.end);
      const { classes } = CodeIndex.parseDocument(fullText);
      const receiver = dotted[1];
      /** @type {string | null} */
      let instClass = null;
      if (receiver === 'this') {
        const ic = CodeIndex.innermostClassAtOffset(fullText, offset);
        instClass = ic && ic.name;
      } else {
        const vmap = CodeIndex.varToClassBeforeOffset(fullText, offset);
        instClass = vmap.get(receiver) || null;
      }
      if (instClass && classes.has(instClass)) {
        const mems = CodeIndex.membersForClass(instClass, classes);
        if (mems.includes(method)) {
          const md = new vscode.MarkdownString(`\`${word}\` — \`${instClass}\` 的成员。`);
          md.isTrusted = true;
          return new vscode.Hover(md, range);
        }
      } else if (instClass) {
        const nsKey = instClass + '.';
        const apis = nsMap[nsKey];
        if (Array.isArray(apis) && apis.includes(method)) {
          const cls = instClass;
          const docKey = `${cls}.${method}`;
          const apiDoc = (CodeAssist.config.apiDocs && CodeAssist.config.apiDocs[docKey]) || '';
          const body = apiDoc ? `${apiDoc}\n\n` : '';
          const md = new vscode.MarkdownString(`${body}\`${word}\` — **${cls}** 内置实例方法。`);
          md.isTrusted = true;
          return new vscode.Hover(md, range);
        }
      }
    }

    return null;
  }

  /**
   * @param {vscode.TextDocument} document
   * @param {vscode.Position} position
   * @returns {vscode.Location[] | undefined}
   */
  static provideDefinition(document, position) {
    const requireLocation = CodeAssist.requireDefinitionLocation(document, position);
    if (requireLocation) {
      return [requireLocation];
    }

    const dotted = CodeAssist.dottedTokenAtCursor(document, position);
    if (dotted) {
      if (dotted.onReceiver) {
        const classLocs = CodeAssist.classDefinitionLocations(document, position, dotted.receiver);
        return classLocs.length ? classLocs : undefined;
      }
      if (dotted.onMember) {
        const memberLocs = CodeAssist.memberDefinitionLocations(
          document,
          position,
          dotted.receiver,
          dotted.member
        );
        return memberLocs.length ? memberLocs : undefined;
      }
      return undefined;
    }

    const wordRange = document.getWordRangeAtPosition(position, /[A-Za-z_][\w.]*/);
    if (!wordRange) {
      return undefined;
    }
    const token = document.getText(wordRange);
    if (!token) {
      return undefined;
    }

    const classLocs = CodeAssist.classDefinitionLocations(document, position, token);
    if (classLocs.length) {
      return classLocs;
    }

    const locations = CodeAssist.symbolDefinitionLocations(document, position, token);
    return locations.length ? locations : undefined;
  }

  static register() {
    CodeAssist.initialize()
      .then(() => {
        const sel = { language: 'boyia' };
        CodeGlobal.context.subscriptions.push(
          vscode.languages.registerCompletionItemProvider(
            sel,
            {
              provideCompletionItems: CodeAssist.provideCompletionItems,
              resolveCompletionItem: CodeAssist.resolveCompletionItem,
            },
            '.',
            '('
          ),
          vscode.languages.registerHoverProvider(sel, {
            provideHover: CodeAssist.provideHover.bind(CodeAssist),
          }),
          vscode.languages.registerDefinitionProvider(sel, {
            provideDefinition: CodeAssist.provideDefinition.bind(CodeAssist),
          }),
          vscode.languages.registerDocumentLinkProvider(sel, {
            provideDocumentLinks: CodeAssist.provideDocumentLinks.bind(CodeAssist),
          })
        );
      })
      .catch((e) => console.error('CodeAssist::register', e));
  }

  /** @deprecated typo — use {@link CodeAssist.register} */
  static reigister() {
    CodeAssist.register();
  }

  static async linkBoyiaFile() {
    const config = vscode.workspace.getConfiguration();
    const associateConfig = config.get('files.associations') || {};

    if (associateConfig['*.boui'] === 'xml' && associateConfig['*.boss'] === 'css') {
      return;
    }

    await config.update(
      'files.associations',
      Object.assign({}, associateConfig, {
        '*.boui': 'xml',
        '*.boss': 'css',
      }),
      vscode.ConfigurationTarget.Global
    );
  }
}

module.exports = CodeAssist;
