---
title: What a BPMN file actually carries, and why it contributes nothing under the knowledge channel
description: Issue #154 step 2, read in ADR 0016's method - a representative BPMN 2.0 model read element by element. BPMN 2.0 models a process, not a semantic model: every flow element carries an id plus free-text name, documentation and proprietary extension properties, and sequence flows reference only elements inside the same process. Nothing names a metric this repository certifies, and the only way to bind a BPMN phrase to a MetricName would be to match harvested free text - which is exactly what can never run at request time, and which at load would certify a source it did not declare. So BPMN contributes nothing under ADR 0036's metric-anchored channel, and issue #154 closes with the record and no crate.
---

# What a BPMN file actually carries, and why it contributes nothing under the knowledge channel

Status: **a spike** (issue #154, step 2). This is the field-by-field reading ADR 0036 sent here to
make: does a BPMN model name a metric this deployment has already certified, in a form that can be
matched at load? The answer is no, for a structural reason that no particular file changes.

## The representative model read

BPMN 2.0 is an XML vocabulary (OMG). A deployment's model is a `bpmn:definitions` document whose core
shape is stable across tools, so a representative model exercises the vocabulary rather than a single
author's habit. The model below is such a representative: an executable process with a lane, a start
event, two tasks, a business rule task, a sequence flow joining them, a data object, a text annotation
and an extension element.

```xml
<bpmn:definitions id="Def_1" targetNamespace="urn:example:bpmn" xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" xmlns:custom="urn:example:ext">
  <bpmn:process id="Process_revenue" name="Month-end revenue review" isExecutable="true">
    <bpmn:laneSet>
      <bpmn:lane id="Lane_finance" name="Finance">
        <bpmn:flowNodeRef>Start_run</bpmn:flowNodeRef>
        <bpmn:flowNodeRef>Task_verify</bpmn:flowNodeRef>
      </bpmn:lane>
    </bpmn:laneSet>
    <bpmn:documentation>Reviews the certified monthly revenue figure before closing.</bpmn:documentation>
    <bpmn:startEvent id="Start_run" name="Start review">
      <bpmn:outgoing>Flow_1</bpmn:outgoing>
    </bpmn:startEvent>
    <bpmn:task id="Task_verify" name="Verify revenue">
      <bpmn:incoming>Flow_1</bpmn:incoming>
      <bpmn:outgoing>Flow_2</bpmn:outgoing>
      <bpmn:extensionElements>
        <custom:property name="metric" value="revenue" />
      </bpmn:extensionElements>
    </bpmn:task>
    <bpmn:businessRuleTask id="Rule_signoff" name="Sign off" implementation="##unspecified">
      <bpmn:incoming>Flow_2</bpmn:incoming>
    </bpmn:businessRuleTask>
    <bpmn:sequenceFlow id="Flow_1" sourceRef="Start_run" targetRef="Task_verify" />
    <bpmn:sequenceFlow id="Flow_2" sourceRef="Task_verify" targetRef="Rule_signoff" />
    <bpmn:dataObject id="Data_result" name="Revenue result" />
    <bpmn:textAnnotation id="Note_revenue">
      <bpmn:text>The revenue metric is net of returns.</bpmn:text>
    </bpmn:textAnnotation>
  </bpmn:process>
</bpmn:definitions>
```

## Field by field, against what the bundle can hold

ADR 0036 settled the channel: a metadata source may contribute knowledge and no definitions, and it
does so through the metric-anchored `Referent` channel - a note, glossary phrase, absence or worked
example attached to a `MetricName` the bundle has already certified, rendered under the metric and
matched **at load**, never at request time. So the spike's question is: what in a BPMN model is a
`MetricName` reference?

| Element                                                              | Carries                                                    | Can it name a certified `MetricName`?                                                        |
| -------------------------------------------------------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `bpmn:process` / `bpmn:lane` / task / event / gateway `@id`, `@name` | free-text label                                            | No - a name is prose, not a reference; nothing in BPMN declares that a name is a metric name |
| `bpmn:sequenceFlow` `sourceRef` / `targetRef`                        | references to **other flow nodes inside the same process** | No - graph topology, not a semantic model                                                    |
| `bpmn:dataObject` / `dataObjectReference`                            | a data element and a name                                  | No - a process variable, not a certified metric                                              |
| `bpmn:documentation` / `bpmn:textAnnotation` / `bpmn:operation` etc. | free text                                                  | No - prose about a process, scoped to nothing the bundle declares                            |
| `bpmn:extensionElements` (`camunda:`/custom properties)              | proprietary, namespace-scoped key/value text               | No - belongs to a tool, not to this bundle's certified vocabulary                            |

The one row that looks hopeful - `extensionElements` carrying a property whose value is the string
`revenue` - is the trap, and it is why "read it at load" is the wrong temptation. The value is free
text authored under a custom namespace with no declared meaning. To bind it to the certified
`MetricName("revenue")` the adapter would have to **match a harvested phrase against the metric
vocabulary** - and that is precisely what is never allowed at request time, and what cannot run at
load either: matching free text to a metric name certifies a binding the BPMN source never declared,
and two deployments could bind the same value `revenue` to two different certified `MetricName`s.
There is no `PhraseNotDefined`; the agent states its own choice, and this repository does not let a
metadata source pre-empt it by guessing.

## The decision, and what happens next

A BPMN model carries process choreography, free-text labels and proprietary tool properties. It has
no element whose content is, or deterministically resolves to, a `MetricName` this bundle certifies.
So under ADR 0036's metric-anchored channel, **BPMN contributes nothing**, and the honest outcome is
the one the record anticipated: no `catalog-bpmn` crate is built, and issue #154 closes with ADR 0036
plus this spike - the same way 0016 closed on a claim that did not survive.

Nothing about this changes if a specific deployment's tooling emits its own extension property that
names a metric: that would be a deployment-defined map on the tool's side, not a load-matchable
BPMN-standard reference, and it would need its own declared mechanism before any adapter could read
it without guessing. That is not this repository's to invent for a source that built no such map.
