// Visual builder for a WorkflowGraph (crates/model/src/workflow.rs), used by
// admin/workflows.html's "Create a workflow definition" form. Vanilla JS, no
// build step or dependencies -- this project ships as one Rust binary with
// no Node toolchain (see README.md's "Frontend" section).
//
// The builder's only job is to keep an in-memory `steps`/`entryStepId` model
// and serialize it into the exact same `graph_json` shape the form already
// posted as a hand-typed JSON textarea, so the server side needed no
// changes at all. The "Advanced: edit as JSON" textarea is that same JSON,
// kept live in sync, with a one-way "Load JSON into builder" action to
// bring hand edits back into the visual model.
(function () {
  'use strict';

  var STEP_KIND_LABELS = {
    question: 'Question',
    branch: 'Branch',
    wait_for_approval: 'Wait for approval',
    submit_ticket: 'Submit ticket',
    end: 'End',
  };
  var INPUT_TYPES = ['text', 'text_area', 'number', 'boolean', 'select', 'multi_select', 'date'];
  var SIMPLE_CONDITION_OPS = ['equals', 'not_equals', 'exists', 'in'];
  var ROLES = ['requester', 'approver', 'agent', 'admin'];
  var END_OUTCOMES = ['completed', 'rejected', 'cancelled'];
  var REF_FIELDS = ['next', 'on_true', 'on_false', 'on_approve', 'on_reject'];
  var NODE_WIDTH = 260;
  var GRID_STEP_X = 300;
  var GRID_STEP_Y = 220;
  var GRID_COLS = 3;

  var steps = [];
  var entryStepId = null;
  var stepCounter = 0;
  var users = [];
  var dragState = null;

  var canvasEl, arrowsContentEl, arrowsSvg, errorEl, graphTextarea, usersSourceEl, fieldKeysDatalist;

  function main() {
    canvasEl = document.getElementById('wf-canvas');
    if (!canvasEl) return; // this page doesn't have the builder

    arrowsSvg = document.getElementById('wf-arrows');
    arrowsContentEl = document.getElementById('wf-arrows-content');
    errorEl = document.getElementById('wf-builder-error');
    graphTextarea = document.getElementById('wf-graph-json');
    usersSourceEl = document.getElementById('wf-users-source');
    fieldKeysDatalist = document.getElementById('wf-field-keys');

    loadUsers();

    document.querySelectorAll('[data-add-step]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        addStep(btn.getAttribute('data-add-step'));
      });
    });

    var loadJsonBtn = document.getElementById('wf-load-json');
    if (loadJsonBtn) loadJsonBtn.addEventListener('click', loadFromTextarea);

    var form = document.getElementById('wf-form');
    if (form) form.addEventListener('submit', onSubmit);

    canvasEl.addEventListener('change', onCanvasChange);
    canvasEl.addEventListener('click', onCanvasClick);
    canvasEl.addEventListener('mousedown', onCanvasMouseDown);

    var prefill = (graphTextarea.value || '').trim();
    var unparseablePrefill = null;
    if (prefill) {
      try {
        hydrateFromGraph(JSON.parse(prefill));
      } catch (err) {
        loadStarterExample();
        unparseablePrefill = prefill;
        showError(
          'Could not load the previously submitted graph JSON (' + err.message + ') into the visual builder -- ' +
          'it has been left as-is in "Advanced: edit as JSON" below so you can fix and reapply it. Showing a ' +
          'blank builder for now.'
        );
      }
    } else {
      loadStarterExample();
    }

    renderAll();
    // renderAll() just overwrote the textarea with the starter example's
    // JSON -- restore the user's actual (unparseable) submission instead,
    // or its content would silently vanish.
    if (unparseablePrefill !== null) {
      graphTextarea.value = unparseablePrefill;
    }
  }

  function loadUsers() {
    if (!usersSourceEl) return;
    users = Array.prototype.map.call(usersSourceEl.options, function (opt) {
      return { value: opt.value, text: opt.textContent };
    });
  }

  function loadStarterExample() {
    stepCounter = 0;
    var q = newStep('question');
    q.id = 'ask_summary';
    q.label = 'What do you need?';
    q.field_key = 'summary';
    q.input_type = 'text_area';
    q.required = true;
    q.next = 'end_ok';
    q.x = 40;
    q.y = 40;

    var e = newStep('end');
    e.id = 'end_ok';
    e.label = 'Done';
    e.outcome = 'completed';
    e.x = 40 + GRID_STEP_X;
    e.y = 40;

    steps = [q, e];
    entryStepId = 'ask_summary';
  }

  // ---------- model ----------

  function newStep(kind) {
    stepCounter += 1;
    var pos = nextPosition(steps.length);
    return {
      _uid: 'n' + stepCounter,
      id: kind + '_' + stepCounter,
      label: '',
      kind: kind,
      x: pos.x,
      y: pos.y,
      // question
      field_key: '',
      input_type: 'text',
      required: true,
      options: [],
      next: '',
      // branch
      condition_op: 'equals',
      condition_field_key: '',
      condition_value_type: 'string',
      condition_value: '',
      condition_values: '',
      _raw_condition: null, // set only when hydrated from an and/or/not condition
      on_true: '',
      on_false: '',
      // wait_for_approval
      approver_type: 'manager_of_requester',
      approver_user_id: '',
      approver_role: 'approver',
      on_approve: '',
      on_reject: '',
      timeout_seconds: '',
      // end
      outcome: 'completed',
    };
  }

  function nextPosition(index) {
    return {
      x: 40 + (index % GRID_COLS) * GRID_STEP_X,
      y: 40 + Math.floor(index / GRID_COLS) * GRID_STEP_Y,
    };
  }

  function addStep(kind) {
    var step = newStep(kind);
    steps.push(step);
    if (!entryStepId) entryStepId = step.id;
    clearError();
    renderAll();
  }

  function outgoingEdges(step) {
    switch (step.kind) {
      case 'question':
        return [{ field: 'next', to: step.next, label: null }];
      case 'branch':
        return [
          { field: 'on_true', to: step.on_true, label: 'true', color: 'green' },
          { field: 'on_false', to: step.on_false, label: 'false', color: 'red' },
        ];
      case 'wait_for_approval':
        return [
          { field: 'on_approve', to: step.on_approve, label: 'approve', color: 'green' },
          { field: 'on_reject', to: step.on_reject, label: 'reject', color: 'red' },
        ];
      case 'submit_ticket':
        return [{ field: 'next', to: step.next, label: null }];
      default:
        return [];
    }
  }

  function findReferencingStepIds(id) {
    var refs = [];
    steps.forEach(function (s) {
      REF_FIELDS.forEach(function (f) {
        if (s[f] === id) refs.push(s.id || '(unnamed step)');
      });
    });
    return refs.filter(function (v, i, arr) {
      return arr.indexOf(v) === i;
    });
  }

  function collectFieldKeys() {
    var keys = steps.filter(function (s) {
      return s.kind === 'question' && s.field_key;
    }).map(function (s) { return s.field_key; });
    return keys.filter(function (v, i, arr) { return arr.indexOf(v) === i; });
  }

  // ---------- serialization: builder state -> WorkflowGraph JSON ----------

  function buildGraphObject() {
    return {
      entry_step_id: entryStepId || '',
      steps: steps.map(stepToJson),
    };
  }

  function stepToJson(step) {
    var out = { id: step.id, label: step.label, kind: null };
    switch (step.kind) {
      case 'question':
        out.kind = {
          type: 'question',
          field_key: step.field_key,
          input_type: step.input_type,
          required: !!step.required,
          options: isChoiceInput(step.input_type) ? step.options.map(function (o) {
            return { value: o.value, label: o.label };
          }) : [],
          next: step.next,
        };
        break;
      case 'branch':
        out.kind = {
          type: 'branch',
          condition: buildConditionJson(step),
          on_true: step.on_true,
          on_false: step.on_false,
        };
        break;
      case 'wait_for_approval':
        out.kind = {
          type: 'wait_for_approval',
          approver_resolution: buildApproverResolutionJson(step),
          on_approve: step.on_approve,
          on_reject: step.on_reject,
          timeout_seconds:
            step.timeout_seconds === '' || step.timeout_seconds === null || step.timeout_seconds === undefined
              ? null
              : Number(step.timeout_seconds),
        };
        break;
      case 'submit_ticket':
        out.kind = { type: 'submit_ticket', next: step.next };
        break;
      case 'end':
        out.kind = { type: 'end', outcome: step.outcome };
        break;
    }
    return out;
  }

  function isChoiceInput(inputType) {
    return inputType === 'select' || inputType === 'multi_select';
  }

  function coerceValue(raw, type) {
    if (type === 'number') return Number(raw);
    if (type === 'boolean') return raw === true || raw === 'true';
    return String(raw);
  }

  function buildConditionJson(step) {
    if (!SIMPLE_CONDITION_OPS.indexOf) {
      // (defensive no-op; Array.prototype.indexOf always exists in any
      // browser this app otherwise supports -- kept simple deliberately)
    }
    if (step._raw_condition && SIMPLE_CONDITION_OPS.indexOf(step.condition_op) === -1) {
      return step._raw_condition;
    }
    if (step.condition_op === 'exists') {
      return { op: 'exists', field_key: step.condition_field_key };
    }
    if (step.condition_op === 'in') {
      var values = (step.condition_values || '')
        .split(',')
        .map(function (s) { return s.trim(); })
        .filter(function (s) { return s.length > 0; })
        .map(function (v) { return coerceValue(v, step.condition_value_type); });
      return { op: 'in', field_key: step.condition_field_key, values: values };
    }
    return {
      op: step.condition_op,
      field_key: step.condition_field_key,
      value: coerceValue(step.condition_value, step.condition_value_type),
    };
  }

  function buildApproverResolutionJson(step) {
    if (step.approver_type === 'static') {
      return { type: 'static', user_id: step.approver_user_id };
    }
    if (step.approver_type === 'role_in_department') {
      return { type: 'role_in_department', role: step.approver_role };
    }
    return { type: 'manager_of_requester' };
  }

  // ---------- hydration: WorkflowGraph JSON -> builder state ----------

  function hydrateFromGraph(graph) {
    if (!graph || !Array.isArray(graph.steps)) {
      throw new Error('expected an object with a "steps" array');
    }
    var newSteps = [];
    stepCounter = 0;
    graph.steps.forEach(function (s, i) {
      var kind = s.kind && s.kind.type;
      if (STEP_KIND_LABELS[kind] === undefined) {
        throw new Error('step "' + (s.id || i) + '" has an unknown kind "' + kind + '"');
      }
      var step = newStep(kind);
      step.id = s.id || step.id;
      step.label = s.label || '';
      var k = s.kind || {};
      switch (kind) {
        case 'question':
          step.field_key = k.field_key || '';
          step.input_type = k.input_type || 'text';
          step.required = !!k.required;
          step.options = (k.options || []).map(function (o) {
            return { value: o.value || '', label: o.label || '' };
          });
          step.next = k.next || '';
          break;
        case 'branch':
          hydrateCondition(step, k.condition);
          step.on_true = k.on_true || '';
          step.on_false = k.on_false || '';
          break;
        case 'wait_for_approval':
          hydrateApproverResolution(step, k.approver_resolution);
          step.on_approve = k.on_approve || '';
          step.on_reject = k.on_reject || '';
          step.timeout_seconds =
            k.timeout_seconds === null || k.timeout_seconds === undefined ? '' : String(k.timeout_seconds);
          break;
        case 'submit_ticket':
          step.next = k.next || '';
          break;
        case 'end':
          step.outcome = k.outcome || 'completed';
          break;
      }
      var pos = nextPosition(i);
      step.x = pos.x;
      step.y = pos.y;
      newSteps.push(step);
    });
    steps = newSteps;
    entryStepId = graph.entry_step_id || (steps[0] && steps[0].id) || null;
  }

  function hydrateCondition(step, condition) {
    condition = condition || { op: 'equals', field_key: '', value: '' };
    if (SIMPLE_CONDITION_OPS.indexOf(condition.op) === -1) {
      // and / or / not -- not editable visually; preserved verbatim.
      step.condition_op = condition.op;
      step._raw_condition = condition;
      return;
    }
    step.condition_op = condition.op;
    step.condition_field_key = condition.field_key || '';
    if (condition.op === 'in') {
      var values = condition.values || [];
      step.condition_value_type = values.length && typeof values[0] === 'number' ? 'number'
        : values.length && typeof values[0] === 'boolean' ? 'boolean'
        : 'string';
      step.condition_values = values.join(', ');
    } else if (condition.op !== 'exists') {
      var v = condition.value;
      step.condition_value_type = typeof v === 'number' ? 'number' : typeof v === 'boolean' ? 'boolean' : 'string';
      step.condition_value = typeof v === 'boolean' ? String(v) : v === undefined || v === null ? '' : String(v);
    }
  }

  function hydrateApproverResolution(step, resolution) {
    resolution = resolution || { type: 'manager_of_requester' };
    step.approver_type = resolution.type || 'manager_of_requester';
    if (step.approver_type === 'static') {
      step.approver_user_id = resolution.user_id || '';
    } else if (step.approver_type === 'role_in_department') {
      step.approver_role = resolution.role || 'approver';
    }
  }

  function loadFromTextarea() {
    var raw = (graphTextarea.value || '').trim();
    if (!raw) {
      showError('The step graph JSON box is empty.');
      return;
    }
    var parsed;
    try {
      parsed = JSON.parse(raw);
    } catch (err) {
      showError('Invalid JSON: ' + err.message);
      return;
    }
    try {
      hydrateFromGraph(parsed);
    } catch (err) {
      showError('Could not load that graph: ' + err.message);
      return;
    }
    clearError();
    renderAll();
  }

  // ---------- rendering ----------

  function renderAll() {
    withFocusPreserved(function () {
      canvasEl.querySelectorAll('.wf-node').forEach(function (el) { el.remove(); });
      var fragment = document.createDocumentFragment();
      steps.forEach(function (step) { fragment.appendChild(buildNodeElement(step)); });
      canvasEl.appendChild(fragment);
    });

    resizeCanvas();
    redrawArrows();
    refreshFieldKeysDatalist();
    syncGraphTextarea();
  }

  function resizeCanvas() {
    var maxX = 900;
    var maxY = 500;
    steps.forEach(function (s) {
      maxX = Math.max(maxX, s.x + NODE_WIDTH + 60);
      maxY = Math.max(maxY, s.y + 260);
    });
    canvasEl.style.width = maxX + 'px';
    canvasEl.style.height = maxY + 'px';
    arrowsSvg.setAttribute('width', String(maxX));
    arrowsSvg.setAttribute('height', String(maxY));
  }

  function buildNodeElement(step) {
    var div = document.createElement('div');
    div.className = 'wf-node wf-node-' + step.kind;
    div.style.left = step.x + 'px';
    div.style.top = step.y + 'px';
    div.setAttribute('data-uid', step._uid);

    var isEntry = !!step.id && step.id === entryStepId;
    div.innerHTML =
      '<div class="wf-node-header">' +
      '<button type="button" class="wf-entry-star' + (isEntry ? ' is-entry' : '') + '" data-action="set-entry" title="' +
      (isEntry ? 'Entry step' : 'Set as entry step') + '">' + (isEntry ? '★' : '☆') + '</button>' +
      '<span class="wf-node-kind">' + escapeHtml(STEP_KIND_LABELS[step.kind]) + '</span>' +
      '<button type="button" class="wf-node-delete" data-action="delete-step" title="Delete step">✕</button>' +
      '</div>' +
      '<div class="wf-node-body">' +
      '<label class="wf-field">Step ID<input type="text" data-field="id" value="' + escapeHtml(step.id) + '"></label>' +
      '<label class="wf-field">Label<input type="text" data-field="label" value="' + escapeHtml(step.label) +
      '" placeholder="Shown to the requester"></label>' +
      buildKindBody(step) +
      '</div>';
    return div;
  }

  function buildKindBody(step) {
    switch (step.kind) {
      case 'question':
        return buildQuestionBody(step);
      case 'branch':
        return buildBranchBody(step);
      case 'wait_for_approval':
        return buildApprovalBody(step);
      case 'submit_ticket':
        return buildRefSelect(step, 'next', 'Next step');
      case 'end':
        return buildEndBody(step);
      default:
        return '';
    }
  }

  function buildQuestionBody(step) {
    var options = INPUT_TYPES.map(function (t) {
      return '<option value="' + t + '"' + (t === step.input_type ? ' selected' : '') + '>' + t + '</option>';
    }).join('');
    var html =
      '<label class="wf-field">Field key<input type="text" data-field="field_key" value="' +
      escapeHtml(step.field_key) + '" placeholder="e.g. summary"></label>' +
      '<label class="wf-field">Input type<select data-field="input_type">' + options + '</select></label>' +
      '<label class="wf-field wf-field-row"><input type="checkbox" data-field="required"' +
      (step.required ? ' checked' : '') + '> Required</label>';
    if (isChoiceInput(step.input_type)) {
      html += buildOptionsEditor(step);
    }
    html += buildRefSelect(step, 'next', 'Next step');
    return html;
  }

  function buildOptionsEditor(step) {
    var rows = step.options.map(function (opt, i) {
      return (
        '<div class="wf-option-row">' +
        '<input type="text" data-field="option_value" data-index="' + i + '" value="' + escapeHtml(opt.value) +
        '" placeholder="value">' +
        '<input type="text" data-field="option_label" data-index="' + i + '" value="' + escapeHtml(opt.label) +
        '" placeholder="label">' +
        '<button type="button" class="link-button" data-action="remove-option" data-index="' + i + '">Remove</button>' +
        '</div>'
      );
    }).join('');
    return (
      '<div class="wf-options"><span class="hint">Options</span>' + rows +
      '<button type="button" class="secondary" data-action="add-option">+ Option</button></div>'
    );
  }

  function buildBranchBody(step) {
    if (SIMPLE_CONDITION_OPS.indexOf(step.condition_op) === -1) {
      return (
        '<p class="hint">Complex condition (<code>' + escapeHtml(step.condition_op) +
        '</code>) -- not editable in the builder; use "Edit as JSON" to change it. The connections below still work.</p>' +
        buildRefSelect(step, 'on_true', 'If true') +
        buildRefSelect(step, 'on_false', 'If false')
      );
    }
    var needsValue = step.condition_op !== 'exists';
    var isIn = step.condition_op === 'in';
    var html =
      '<label class="wf-field">Field key<input type="text" data-field="condition_field_key" value="' +
      escapeHtml(step.condition_field_key) + '" list="wf-field-keys"></label>' +
      '<label class="wf-field">Operator<select data-field="condition_op">' +
      opt('equals', step.condition_op, 'equals') +
      opt('not_equals', step.condition_op, 'not equals') +
      opt('exists', step.condition_op, 'has an answer') +
      opt('in', step.condition_op, 'is one of') +
      '</select></label>';
    if (needsValue) {
      html +=
        '<label class="wf-field">Value type<select data-field="condition_value_type">' +
        opt('string', step.condition_value_type, 'text') +
        opt('number', step.condition_value_type, 'number') +
        opt('boolean', step.condition_value_type, 'true/false') +
        '</select></label>' +
        '<label class="wf-field">' + (isIn ? 'Values (comma-separated)' : 'Value') +
        buildConditionValueInput(step, isIn) + '</label>';
    }
    html += buildRefSelect(step, 'on_true', 'If true') + buildRefSelect(step, 'on_false', 'If false');
    return html;
  }

  function buildConditionValueInput(step, isIn) {
    if (isIn) {
      return '<input type="text" data-field="condition_values" value="' + escapeHtml(step.condition_values) +
        '" placeholder="a, b, c">';
    }
    if (step.condition_value_type === 'boolean') {
      return '<select data-field="condition_value">' +
        opt('true', step.condition_value, 'true') +
        opt('false', step.condition_value, 'false') +
        '</select>';
    }
    var inputType = step.condition_value_type === 'number' ? 'number' : 'text';
    return '<input type="' + inputType + '" data-field="condition_value" value="' + escapeHtml(step.condition_value) + '">';
  }

  function buildApprovalBody(step) {
    var html =
      '<label class="wf-field">Approver<select data-field="approver_type">' +
      opt('manager_of_requester', step.approver_type, "Requester's manager") +
      opt('role_in_department', step.approver_type, "Anyone with a role, in the requester's department") +
      opt('static', step.approver_type, 'A specific person') +
      '</select></label>';
    if (step.approver_type === 'role_in_department') {
      html += '<label class="wf-field">Role<select data-field="approver_role">' +
        ROLES.map(function (r) { return opt(r, step.approver_role, r); }).join('') +
        '</select></label>';
    }
    if (step.approver_type === 'static') {
      html += '<label class="wf-field">Person<select data-field="approver_user_id">' +
        '<option value="">-- choose --</option>' +
        users.map(function (u) { return opt(u.value, step.approver_user_id, u.text); }).join('') +
        '</select></label>';
    }
    html += '<label class="wf-field">Auto-reject after (seconds, optional)' +
      '<input type="number" min="0" data-field="timeout_seconds" value="' + escapeHtml(step.timeout_seconds) + '"></label>';
    html += buildRefSelect(step, 'on_approve', 'If approved') + buildRefSelect(step, 'on_reject', 'If rejected');
    return html;
  }

  function buildEndBody(step) {
    return '<label class="wf-field">Outcome<select data-field="outcome">' +
      END_OUTCOMES.map(function (o) { return opt(o, step.outcome, o); }).join('') +
      '</select></label>';
  }

  function buildRefSelect(step, fieldName, labelText) {
    var current = step[fieldName];
    var exists = steps.some(function (s) { return s.id === current; });
    var options = '<option value="">-- choose --</option>';
    if (current && !exists) {
      options += '<option value="' + escapeHtml(current) + '" selected>(missing) ' + escapeHtml(current) + '</option>';
    }
    options += steps.filter(function (s) { return !!s.id; }).map(function (s) {
      var text = s.id + (s.label ? ' — ' + s.label : '');
      return '<option value="' + escapeHtml(s.id) + '"' + (s.id === current ? ' selected' : '') + '>' +
        escapeHtml(text) + '</option>';
    }).join('');
    return '<label class="wf-field">' + escapeHtml(labelText) + '<select data-field="' + fieldName + '">' +
      options + '</select></label>';
  }

  function opt(value, current, text) {
    return '<option value="' + escapeHtml(value) + '"' + (value === current ? ' selected' : '') + '>' +
      escapeHtml(text) + '</option>';
  }

  function refreshFieldKeysDatalist() {
    if (!fieldKeysDatalist) return;
    fieldKeysDatalist.innerHTML = collectFieldKeys().map(function (k) {
      return '<option value="' + escapeHtml(k) + '">';
    }).join('');
  }

  function redrawArrows() {
    while (arrowsContentEl.firstChild) arrowsContentEl.removeChild(arrowsContentEl.firstChild);
    steps.forEach(function (step) {
      var fromEl = canvasEl.querySelector('[data-uid="' + step._uid + '"]');
      if (!fromEl) return;
      var from = { x: step.x, y: step.y, w: fromEl.offsetWidth, h: fromEl.offsetHeight };
      outgoingEdges(step).forEach(function (edge) {
        if (!edge.to) return;
        var target = steps.find(function (s) { return s.id === edge.to; });
        if (!target) return;
        var toEl = canvasEl.querySelector('[data-uid="' + target._uid + '"]');
        if (!toEl) return;
        var to = { x: target.x, y: target.y, w: toEl.offsetWidth, h: toEl.offsetHeight };
        drawArrow(from, to, edge.label, edge.color);
      });
    });
  }

  function drawArrow(from, to, label, color) {
    var x1 = from.x + from.w / 2;
    var y1 = from.y + from.h;
    var x2 = to.x + to.w / 2;
    var y2 = to.y;
    var midY = (y1 + y2) / 2;
    var path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    path.setAttribute('d', 'M ' + x1 + ' ' + y1 + ' C ' + x1 + ' ' + midY + ', ' + x2 + ' ' + midY + ', ' + x2 + ' ' + y2);
    path.setAttribute('class', 'wf-arrow' + (color ? ' wf-arrow-' + color : ''));
    path.setAttribute('marker-end', 'url(#wf-arrowhead' + (color ? '-' + color : '') + ')');
    arrowsContentEl.appendChild(path);

    if (label) {
      var text = document.createElementNS('http://www.w3.org/2000/svg', 'text');
      text.setAttribute('x', String((x1 + x2) / 2 + 6));
      text.setAttribute('y', String(midY));
      text.setAttribute('class', 'wf-arrow-label' + (color ? ' wf-arrow-label-' + color : ''));
      text.textContent = label;
      arrowsContentEl.appendChild(text);
    }
  }

  function syncGraphTextarea() {
    graphTextarea.value = JSON.stringify(buildGraphObject(), null, 2);
  }

  // ---------- events ----------

  function findStepByUid(uid) {
    return steps.find(function (s) { return s._uid === uid; });
  }

  function onCanvasChange(event) {
    var target = event.target;
    var field = target.getAttribute('data-field');
    if (!field) return;
    var nodeEl = target.closest('[data-uid]');
    if (!nodeEl) return;
    var step = findStepByUid(nodeEl.getAttribute('data-uid'));
    if (!step) return;

    if (field === 'id') {
      commitIdChange(step, target.value.trim());
    } else if (field === 'option_value' || field === 'option_label') {
      var idx = Number(target.getAttribute('data-index'));
      var key = field === 'option_value' ? 'value' : 'label';
      if (step.options[idx]) step.options[idx][key] = target.value;
    } else if (target.type === 'checkbox') {
      step[field] = target.checked;
    } else {
      step[field] = target.value;
    }
    renderAll();
  }

  function commitIdChange(step, newId) {
    var oldId = step.id;
    if (!newId) {
      showError('A step id cannot be blank.');
      return;
    }
    if (newId === oldId) return;
    if (steps.some(function (s) { return s !== step && s.id === newId; })) {
      showError('Step id "' + newId + '" is already used by another step -- choose a different one.');
      return;
    }
    clearError();
    steps.forEach(function (s) {
      REF_FIELDS.forEach(function (f) {
        if (s[f] === oldId) s[f] = newId;
      });
    });
    if (entryStepId === oldId) entryStepId = newId;
    step.id = newId;
  }

  function onCanvasClick(event) {
    var btn = event.target.closest('[data-action]');
    if (!btn) return;
    var action = btn.getAttribute('data-action');
    var nodeEl = btn.closest('[data-uid]');
    var step = nodeEl && findStepByUid(nodeEl.getAttribute('data-uid'));

    if (action === 'delete-step' && step) {
      var refs = findReferencingStepIds(step.id);
      if (refs.length &&
        !window.confirm(
          'This step is referenced by: ' + refs.join(', ') +
          '. Delete it anyway? Those connections will point at a step that no longer exists until you fix them.'
        )
      ) {
        return;
      }
      steps = steps.filter(function (s) { return s !== step; });
      if (entryStepId === step.id) entryStepId = steps.length ? steps[0].id : null;
    } else if (action === 'set-entry' && step) {
      entryStepId = step.id;
    } else if (action === 'add-option' && step) {
      step.options.push({ value: '', label: '' });
    } else if (action === 'remove-option' && step) {
      step.options.splice(Number(btn.getAttribute('data-index')), 1);
    } else {
      return;
    }
    clearError();
    renderAll();
  }

  function onCanvasMouseDown(event) {
    var header = event.target.closest('.wf-node-header');
    if (!header || event.target.closest('button')) return;
    var nodeEl = header.closest('[data-uid]');
    var step = nodeEl && findStepByUid(nodeEl.getAttribute('data-uid'));
    if (!step) return;

    event.preventDefault();
    var canvasRect = canvasEl.getBoundingClientRect();
    dragState = {
      step: step,
      el: nodeEl,
      offsetX: event.clientX - canvasRect.left - step.x,
      offsetY: event.clientY - canvasRect.top - step.y,
    };
    document.addEventListener('mousemove', onDragMove);
    document.addEventListener('mouseup', onDragEnd);
  }

  function onDragMove(event) {
    if (!dragState) return;
    var canvasRect = canvasEl.getBoundingClientRect();
    var x = Math.max(0, event.clientX - canvasRect.left - dragState.offsetX);
    var y = Math.max(0, event.clientY - canvasRect.top - dragState.offsetY);
    dragState.step.x = x;
    dragState.step.y = y;
    dragState.el.style.left = x + 'px';
    dragState.el.style.top = y + 'px';
    resizeCanvas();
    redrawArrows();
  }

  function onDragEnd() {
    dragState = null;
    document.removeEventListener('mousemove', onDragMove);
    document.removeEventListener('mouseup', onDragEnd);
    syncGraphTextarea();
  }

  function withFocusPreserved(fn) {
    var active = document.activeElement;
    var selector = null;
    if (active && canvasEl.contains(active)) {
      var nodeEl = active.closest('[data-uid]');
      var field = active.getAttribute('data-field');
      var index = active.getAttribute('data-index');
      if (nodeEl && field) {
        selector = '[data-uid="' + nodeEl.getAttribute('data-uid') + '"] [data-field="' + field + '"]' +
          (index !== null ? '[data-index="' + index + '"]' : '');
      }
    }
    fn();
    if (selector) {
      var el = canvasEl.querySelector(selector);
      if (el) el.focus();
    }
  }

  // ---------- validation + submit ----------

  function validate() {
    var errors = [];
    if (!steps.length) {
      return ['Add at least one step.'];
    }
    if (!entryStepId || !steps.some(function (s) { return s.id === entryStepId; })) {
      errors.push('Mark one step as the entry step (click the star in its header).');
    }
    var seen = {};
    var ids = steps.map(function (s) { return s.id; });
    steps.forEach(function (s) {
      if (!s.id) {
        errors.push('Every step needs a non-empty id.');
      } else if (seen[s.id]) {
        errors.push('Step id "' + s.id + '" is used more than once.');
      }
      seen[s.id] = true;

      outgoingEdges(s).forEach(function (edge) {
        if (!edge.to) {
          errors.push('Step "' + s.id + '" is missing a connection.');
        } else if (ids.indexOf(edge.to) === -1) {
          errors.push('Step "' + s.id + '" points to "' + edge.to + '", which doesn\'t exist.');
        }
      });

      if (s.kind === 'question' && !s.field_key) {
        errors.push('Question step "' + s.id + '" needs a field key.');
      }
      if (s.kind === 'branch' && SIMPLE_CONDITION_OPS.indexOf(s.condition_op) !== -1 && !s.condition_field_key) {
        errors.push('Branch step "' + s.id + '" needs a field key to check.');
      }
      if (s.kind === 'wait_for_approval' && s.approver_type === 'static' && !s.approver_user_id) {
        errors.push('Approval step "' + s.id + '" needs a specific person selected.');
      }
    });
    return errors.filter(function (v, i, arr) { return arr.indexOf(v) === i; });
  }

  function onSubmit(event) {
    var errors = validate();
    if (errors.length) {
      event.preventDefault();
      showError(errors.join(' '));
      canvasEl.scrollIntoView({ behavior: 'smooth', block: 'center' });
      return;
    }
    clearError();
    syncGraphTextarea();
  }

  function showError(message) {
    errorEl.textContent = message;
    errorEl.hidden = false;
  }

  function clearError() {
    errorEl.hidden = true;
    errorEl.textContent = '';
  }

  function escapeHtml(value) {
    return String(value === null || value === undefined ? '' : value).replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', main);
  } else {
    main();
  }
})();
