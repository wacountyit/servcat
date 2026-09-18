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
//
// Steps render as an ordinary vertical list in normal document flow --
// deliberately not an absolutely-positioned free-drag canvas. That means
// the list's own height always contains its content correctly with no
// manual sizing/overflow math, and a connector between two adjacent cards
// is always a tiny, fixed-size straight line (never a path computed across
// an arbitrary, unbounded distance).
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

  var steps = [];
  var entryStepId = null;
  var stepCounter = 0;
  var users = [];

  var stepsListEl, errorEl, graphTextarea, usersSourceEl, fieldKeysDatalist;

  function main() {
    stepsListEl = document.getElementById('wf-steps');
    if (!stepsListEl) return; // this page doesn't have the builder

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

    stepsListEl.addEventListener('change', onListChange);
    stepsListEl.addEventListener('click', onListClick);

    var prefill = (graphTextarea.value || '').trim();
    var unparseablePrefill = null;
    var loadErrorMessage = null;
    if (prefill) {
      try {
        hydrateFromGraph(JSON.parse(prefill));
      } catch (err) {
        loadStarterExample();
        unparseablePrefill = prefill;
        loadErrorMessage =
          'Could not load the previously submitted graph JSON (' + err.message + ') into the visual builder -- ' +
          'it has been left as-is in "Advanced: edit as JSON" below so you can fix and reapply it. Showing a ' +
          'blank builder for now.';
      }
    } else {
      loadStarterExample();
    }

    // renderAll() re-syncs the JSON textarea from the (now-loaded) builder
    // state and updates the live validation status -- both need to happen
    // before restoring the unparseable text / the load-failure message, or
    // they'd immediately get overwritten by it.
    renderAll();
    if (unparseablePrefill !== null) {
      graphTextarea.value = unparseablePrefill;
      showError(loadErrorMessage);
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

    var e = newStep('end');
    e.id = 'end_ok';
    e.label = 'Done';
    e.outcome = 'completed';

    steps = [q, e];
    entryStepId = 'ask_summary';
  }

  // ---------- model ----------

  function newStep(kind) {
    stepCounter += 1;
    return {
      _uid: 'n' + stepCounter,
      _collapsed: false,
      id: kind + '_' + stepCounter,
      label: '',
      kind: kind,
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

  function addStep(kind) {
    var step = newStep(kind);
    steps.push(step);
    if (!entryStepId) entryStepId = step.id;
    clearError();
    renderAll();
    var el = stepsListEl.querySelector('[data-uid="' + step._uid + '"]');
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'center' });
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
      stepsListEl.innerHTML = '';
      if (!steps.length) {
        var empty = document.createElement('p');
        empty.className = 'wf-steps-empty hint';
        empty.id = 'wf-steps-empty';
        empty.textContent = 'No steps yet -- add one above to get started.';
        stepsListEl.appendChild(empty);
      } else {
        steps.forEach(function (step, i) {
          stepsListEl.appendChild(buildStepCard(step, i));
          if (i < steps.length - 1) {
            var edge = findConnectingEdge(step, steps[i + 1].id);
            if (edge) stepsListEl.appendChild(buildConnector(edge));
          }
        });
      }
    });

    refreshFieldKeysDatalist();
    syncGraphTextarea();
    renderValidationStatus();
  }

  // The edge (if any) from `step` that leads directly into `nextId` -- used
  // to decide whether a connector is drawn between two *adjacent* cards.
  // A step's other edges (if it has more than one, or if it points
  // somewhere further down/up the list) are still fully wired up via that
  // step's own dropdowns; they're just not drawn as a line here.
  function findConnectingEdge(step, nextId) {
    var edges = outgoingEdges(step);
    for (var i = 0; i < edges.length; i++) {
      if (edges[i].to === nextId) return edges[i];
    }
    return null;
  }

  function buildConnector(edge) {
    var wrap = document.createElement('div');
    wrap.className = 'wf-connector' + (edge.color ? ' wf-connector-' + edge.color : '');
    var markerId = 'wf-arrowhead' + (edge.color ? '-' + edge.color : '');
    wrap.innerHTML =
      '<svg width="16" height="26" viewBox="0 0 16 26" aria-hidden="true" focusable="false">' +
      '<path d="M8,1 L8,18" class="wf-connector-line" fill="none" marker-end="url(#' + markerId + ')"></path>' +
      '</svg>' +
      (edge.label ? '<span>' + escapeHtml(edge.label) + '</span>' : '');
    return wrap;
  }

  function buildStepCard(step, index) {
    var div = document.createElement('div');
    div.className = 'wf-step-card wf-step-' + step.kind + (step._collapsed ? ' wf-step-collapsed' : '');
    div.setAttribute('data-uid', step._uid);

    var isEntry = !!step.id && step.id === entryStepId;
    var isFirst = index === 0;
    var isLast = index === steps.length - 1;

    div.innerHTML =
      '<div class="wf-step-header">' +
      '<button type="button" class="wf-icon-btn" data-action="toggle-collapse" ' +
      'aria-expanded="' + (!step._collapsed) + '" title="' + (step._collapsed ? 'Expand step' : 'Collapse step') + '">' +
      (step._collapsed ? '▸' : '▾') + '</button>' +
      '<span class="wf-step-badge">' + escapeHtml(STEP_KIND_LABELS[step.kind]) + '</span>' +
      '<input type="text" data-field="label" value="' + escapeHtml(step.label) +
      '" placeholder="Step label" aria-label="Step label">' +
      '<button type="button" class="wf-icon-btn" data-action="move-up" title="Move up"' +
      (isFirst ? ' disabled' : '') + '>↑</button>' +
      '<button type="button" class="wf-icon-btn" data-action="move-down" title="Move down"' +
      (isLast ? ' disabled' : '') + '>↓</button>' +
      '<button type="button" class="wf-icon-btn wf-entry-star' + (isEntry ? ' is-entry' : '') + '" ' +
      'data-action="set-entry" title="' + (isEntry ? 'Entry step' : 'Set as entry step') + '">' +
      (isEntry ? '★' : '☆') + '</button>' +
      '<button type="button" class="wf-icon-btn wf-step-delete" data-action="delete-step" title="Delete step">✕</button>' +
      '</div>' +
      '<div class="wf-step-body">' +
      '<label class="wf-form-group">Step ID<input type="text" data-field="id" value="' + escapeHtml(step.id) + '"></label>' +
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
      '<label class="wf-form-group">Field key<input type="text" data-field="field_key" value="' +
      escapeHtml(step.field_key) + '" placeholder="e.g. summary"></label>' +
      '<label class="wf-form-group">Input type<select data-field="input_type">' + options + '</select></label>' +
      '<label class="wf-form-group wf-form-row"><input type="checkbox" data-field="required"' +
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
        '" placeholder="value" aria-label="Option value">' +
        '<input type="text" data-field="option_label" data-index="' + i + '" value="' + escapeHtml(opt.label) +
        '" placeholder="label" aria-label="Option label">' +
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
      '<label class="wf-form-group">Field key<input type="text" data-field="condition_field_key" value="' +
      escapeHtml(step.condition_field_key) + '" list="wf-field-keys"></label>' +
      '<label class="wf-form-group">Operator<select data-field="condition_op">' +
      opt('equals', step.condition_op, 'equals') +
      opt('not_equals', step.condition_op, 'not equals') +
      opt('exists', step.condition_op, 'has an answer') +
      opt('in', step.condition_op, 'is one of') +
      '</select></label>';
    if (needsValue) {
      html +=
        '<label class="wf-form-group">Value type<select data-field="condition_value_type">' +
        opt('string', step.condition_value_type, 'text') +
        opt('number', step.condition_value_type, 'number') +
        opt('boolean', step.condition_value_type, 'true/false') +
        '</select></label>' +
        '<label class="wf-form-group">' + (isIn ? 'Values (comma-separated)' : 'Value') +
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
      '<label class="wf-form-group">Approver<select data-field="approver_type">' +
      opt('manager_of_requester', step.approver_type, "Requester's manager") +
      opt('role_in_department', step.approver_type, "Anyone with a role, in the requester's department") +
      opt('static', step.approver_type, 'A specific person') +
      '</select></label>';
    if (step.approver_type === 'role_in_department') {
      html += '<label class="wf-form-group">Role<select data-field="approver_role">' +
        ROLES.map(function (r) { return opt(r, step.approver_role, r); }).join('') +
        '</select></label>';
    }
    if (step.approver_type === 'static') {
      html += '<label class="wf-form-group">Person<select data-field="approver_user_id">' +
        '<option value="">-- choose --</option>' +
        users.map(function (u) { return opt(u.value, step.approver_user_id, u.text); }).join('') +
        '</select></label>';
    }
    html += '<label class="wf-form-group">Auto-reject after (seconds, optional)' +
      '<input type="number" min="0" data-field="timeout_seconds" value="' + escapeHtml(step.timeout_seconds) + '"></label>';
    html += buildRefSelect(step, 'on_approve', 'If approved') + buildRefSelect(step, 'on_reject', 'If rejected');
    return html;
  }

  function buildEndBody(step) {
    return '<label class="wf-form-group">Outcome<select data-field="outcome">' +
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
    return '<label class="wf-form-group">' + escapeHtml(labelText) + '<select data-field="' + fieldName + '">' +
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

  function syncGraphTextarea() {
    graphTextarea.value = JSON.stringify(buildGraphObject(), null, 2);
  }

  // ---------- events ----------

  function findStepByUid(uid) {
    return steps.find(function (s) { return s._uid === uid; });
  }

  function onListChange(event) {
    var target = event.target;
    var field = target.getAttribute('data-field');
    if (!field) return;
    var cardEl = target.closest('[data-uid]');
    if (!cardEl) return;
    var step = findStepByUid(cardEl.getAttribute('data-uid'));
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

  function onListClick(event) {
    var btn = event.target.closest('[data-action]');
    if (!btn || btn.disabled) return;
    var action = btn.getAttribute('data-action');
    var cardEl = btn.closest('[data-uid]');
    var step = cardEl && findStepByUid(cardEl.getAttribute('data-uid'));
    if (!step) return;
    var index = steps.indexOf(step);

    if (action === 'delete-step') {
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
    } else if (action === 'set-entry') {
      entryStepId = step.id;
    } else if (action === 'add-option') {
      step.options.push({ value: '', label: '' });
    } else if (action === 'remove-option') {
      step.options.splice(Number(btn.getAttribute('data-index')), 1);
    } else if (action === 'toggle-collapse') {
      step._collapsed = !step._collapsed;
    } else if (action === 'move-up' && index > 0) {
      steps.splice(index - 1, 0, steps.splice(index, 1)[0]);
    } else if (action === 'move-down' && index < steps.length - 1) {
      steps.splice(index + 1, 0, steps.splice(index, 1)[0]);
    } else {
      return;
    }
    clearError();
    renderAll();
  }

  // Keeps keyboard focus on "the same control" across a full re-render --
  // otherwise every click/change (including repeatedly pressing a
  // Move up/down or Collapse button) would drop focus back to the
  // document body, forcing a keyboard user to tab back in each time.
  // If the step itself was just deleted, there's nothing sensible to
  // restore focus to, so it's simply left wherever the browser puts it.
  function withFocusPreserved(fn) {
    var active = document.activeElement;
    var selector = null;
    if (active && stepsListEl.contains(active)) {
      var cardEl = active.closest('[data-uid]');
      var field = active.getAttribute('data-field');
      var action = active.getAttribute('data-action');
      var index = active.getAttribute('data-index');
      if (cardEl && field) {
        selector = '[data-uid="' + cardEl.getAttribute('data-uid') + '"] [data-field="' + field + '"]' +
          (index !== null ? '[data-index="' + index + '"]' : '');
      } else if (cardEl && action) {
        selector = '[data-uid="' + cardEl.getAttribute('data-uid') + '"] [data-action="' + action + '"]';
      }
    }
    fn();
    if (selector) {
      var el = stepsListEl.querySelector(selector);
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
    if (!steps.some(function (s) { return s.kind === 'end'; })) {
      errors.push('Add at least one End step, or a request could never actually finish.');
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

  // Keeps the status box live-updated with the current validation state on
  // every render, rather than only ever checking at submit time -- so a
  // problem is visible (and fixable) the moment it's introduced instead of
  // failing silently until the form is submitted. Shown as a mild warning
  // (not the harsher error style) since mid-construction a graph is
  // *expected* to be temporarily incomplete -- e.g. right after adding a
  // step and before wiring it up -- and a red alert on every keystroke
  // would be more naggy than helpful.
  function renderValidationStatus() {
    var errors = validate();
    if (errors.length) {
      showStatus(errors.join(' '), 'warning');
    } else {
      clearError();
    }
  }

  function onSubmit(event) {
    var errors = validate();
    if (errors.length) {
      event.preventDefault();
      showError(errors.join(' '));
      errorEl.scrollIntoView({ behavior: 'smooth', block: 'center' });
      return;
    }
    clearError();
    syncGraphTextarea();
  }

  function showError(message) {
    showStatus(message, 'error');
  }

  function showStatus(message, kind) {
    errorEl.textContent = message;
    errorEl.hidden = false;
    errorEl.classList.remove('alert-error', 'alert-warning');
    errorEl.classList.add(kind === 'warning' ? 'alert-warning' : 'alert-error');
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
