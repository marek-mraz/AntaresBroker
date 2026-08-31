*** Settings ***
Documentation       Distributed temporal query inputs (Antares extension
...                 IOP TPs), one edge per 5.7.4.3 input over a real remote
...                 series: the 4.15 language filter (Tables 6.18.3.2-1 /
...                 6.19.3.1 `lang`) on remote LanguageProperty instances;
...                 the values filter "checked against all the Attribute
...                 instances resulting from the initial filtering
...                 performed by the temporal query" (5.7.4.4 S2); the
...                 Context Source filter (5.7.4.4: "the same Context
...                 Source filter input parameter ... shall be propagated");
...                 the queryTemporal operation gate (5.7.4.4, 4.20); the
...                 5.9.1 interval rule ("If the timeproperty is createdAt,
...                 modifiedAt or deletedAt, the temporal query is matched
...                 against the managementInterval ... If the relevant
...                 interval is not present, there is no match"); pick/omit
...                 (5.7.4.5) and the Attribute-list selection ("of which at
...                 least one shall exist in order for an Entity to be
...                 selected") on the merged result.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             RequestsLibrary

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}
${TEMPORAL_OPS}=    ${{ ["retrieveTemporal", "queryTemporal"] }}
${WINDOW_AFTER}=    2020-01-01T00:00:00Z


*** Test Cases ***
IOP_EXT_TMP_04_01 Lang Reduces A Remote LanguageProperty Series
    [Documentation]    Table 6.18.3.2-1 `lang` applied by B1 to instances
    ...    that live only in B2: every instance is a Property in French
    ...    carrying `lang`, no languageMap survives the merge.
    [Tags]    iop    iop-ext    5_7_4    4_15    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "label": [{"type": "LanguageProperty", "languageMap": {"en": "hello", "fr": "bonjour"}, "observedAt": "2026-05-01T00:00:00Z"}, {"type": "LanguageProperty", "languageMap": {"en": "bye", "fr": "salut"}, "observedAt": "2026-05-02T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${response}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=${WINDOW_AFTER}    lang=fr
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}
    ${instances}=    Label Instances    ${response.json()[0]}
    Length Should Be    ${instances}    2
    FOR    ${inst}    IN    @{instances}
        Should Be Equal    ${inst}[type]    Property
        Should Be Equal    ${inst}[lang]    fr
        Should Be True    $inst['value'] in ('bonjour', 'salut')
    END
    Should Not Contain    ${response.text}    languageMap
    Should Not Contain    ${response.text}    hello

IOP_EXT_TMP_04_02 Lang Reduces A Remote Evolution On Retrieve
    [Documentation]    Table 6.19.3.1 `lang` on GET /temporal/entities/{id}
    ...    served through B1 from B2's series — and in the temporalValues
    ...    form the reduced Property renders as 4.5.9 `values` pairs.
    [Tags]    iop    iop-ext    5_7_3    4_15    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "label": [{"type": "LanguageProperty", "languageMap": {"en": "hello", "fr": "bonjour"}, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${normalized}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after    timeAt=${WINDOW_AFTER}    lang=en
    Check Response Status Code    200    ${normalized.status_code}
    ${instances}=    Label Instances    ${normalized.json()}
    Length Should Be    ${instances}    1
    Should Be Equal    ${instances}[0][type]    Property
    Should Be Equal    ${instances}[0][value]    hello
    Should Be Equal    ${instances}[0][lang]    en
    Should Not Contain    ${normalized.text}    languageMap

    ${simplified}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after    timeAt=${WINDOW_AFTER}    lang=en    format=temporalValues
    Check Response Status Code    200    ${simplified.status_code}
    ${label}=    Set Variable    ${simplified.json()}[label]
    Should Be Equal    ${label}[type]    Property
    Should Be Equal    ${label}[values][0][0]    hello
    Dictionary Should Not Contain Key    ${label}    languageMaps

IOP_EXT_TMP_04_03 The Values Filter Is Judged On The Windowed Remote Instances
    [Documentation]    5.7.4.4 S2: q is "checked against all the Attribute
    ...    instances resulting from the initial filtering performed by the
    ...    temporal query" — a remote value outside the window cannot
    ...    satisfy q, a remote value inside it can.
    [Tags]    iop    iop-ext    5_7_4    4_9    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}, {"type": "Property", "value": 90, "observedAt": "2026-06-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${outside}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=between    timeAt=2026-04-15T00:00:00Z    endTimeAt=2026-05-15T00:00:00Z
    ...    q=speed>50
    Check Response Status Code    200    ${outside.status_code}
    Should Not Contain    ${outside.text}    ${entity_id}
    ${inside}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=between    timeAt=2026-04-15T00:00:00Z    endTimeAt=2026-05-15T00:00:00Z
    ...    q=speed<50
    Check Response Status Code    200    ${inside.status_code}
    Should Contain    ${inside.text}    ${entity_id}
    Should Contain    ${inside.text}    "value":10
    Should Not Contain    ${inside.text}    "value":90

IOP_EXT_TMP_04_04 Csf Gates The Temporal Context Sources
    [Documentation]    5.7.4.3 Context Source filter over the registration's
    ...    own Context Source Properties: B2's registration carries
    ...    sourceType="sensor", B3's does not — only B2's series is
    ...    consulted for the temporal query.
    [Tags]    iop    iop-ext    5_7_4    4_9    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint, "operations": ["retrieveTemporal", "queryTemporal"], "sourceType": {"type": "Property", "value": "sensor"}}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b3_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    FOR    ${broker}    ${tail}    IN    ${b2_url}    -b2    ${b3_url}    -b3
        ${doc}=    Evaluate
        ...    {"id": $entity_id + $tail, "type": $etype, "speed": [{"type": "Property", "value": 1, "observedAt": "2026-05-01T00:00:00Z"}]}
        Upsert Temporal At Broker    ${broker}    ${doc}
    END

    ${filtered}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=${WINDOW_AFTER}    csf=sourceType=="sensor"
    Check Response Status Code    200    ${filtered.status_code}
    Should Contain    ${filtered.text}    ${entity_id}-b2
    Should Not Contain    ${filtered.text}    ${entity_id}-b3
    ${all}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=${WINDOW_AFTER}
    Check Response Status Code    200    ${all.status_code}
    Should Contain    ${all.text}    ${entity_id}-b2
    Should Contain    ${all.text}    ${entity_id}-b3

IOP_EXT_TMP_04_05 Only Registrations Supporting queryTemporal Are Queried
    [Documentation]    5.7.4.4: "for Context Source Registrations that match
    ...    the query and support the queryTemporal operation" — a
    ...    registration declaring only retrieveTemporal serves the retrieve
    ...    form through B1 but is never consulted by the query form.
    [Tags]    iop    iop-ext    5_7_4    4_20    since_v1.9.1
    ${retrieve_only}=    Evaluate    ["retrieveTemporal"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${retrieve_only}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 7, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${retrieve}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after    timeAt=${WINDOW_AFTER}
    Check Response Status Code    200    ${retrieve.status_code}
    Should Contain    ${retrieve.text}    "value":7
    ${query}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=${WINDOW_AFTER}
    Check Response Status Code    200    ${query.status_code}
    Should Not Contain    ${query.text}    ${entity_id}

IOP_EXT_TMP_04_06 managementInterval Answers Only Management Timeproperties
    [Documentation]    5.9.1: a registration with a managementInterval and
    ...    no observationInterval matches a modifiedAt-based temporal query
    ...    (open-ended interval) but never the default observedAt one —
    ...    "If the relevant interval is not present, there is no match".
    [Tags]    iop    iop-ext    5_9_1    5_2_9    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint, "operations": ["retrieveTemporal", "queryTemporal"], "managementInterval": {"startAt": "2020-01-01T00:00:00Z"}}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 21, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${observed}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=${WINDOW_AFTER}
    Check Response Status Code    200    ${observed.status_code}
    Should Not Contain    ${observed.text}    ${entity_id}
    ${modified}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=${WINDOW_AFTER}    timeproperty=modifiedAt
    Check Response Status Code    200    ${modified.status_code}
    Should Contain    ${modified.text}    ${entity_id}
    Should Contain    ${modified.text}    "value":21

IOP_EXT_TMP_04_07 Pick And Omit Reduce The Merged Temporal Entity
    [Documentation]    5.7.4.5: "If a restrictive list of Entity member
    ...    names is present, every Entity ... is reduced down to only
    ...    contain the defined Entity members"; the exclusionary list
    ...    removes them — applied on the result merged from B2.
    [Tags]    iop    iop-ext    5_7_4    4_21    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 3, "observedAt": "2026-05-01T00:00:00Z"}], "brand": [{"type": "Property", "value": "Acme", "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${picked}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=${WINDOW_AFTER}    pick=id,type,speed
    Check Response Status Code    200    ${picked.status_code}
    Dictionary Should Contain Key    ${picked.json()[0]}    speed
    Dictionary Should Not Contain Key    ${picked.json()[0]}    brand
    ${omitted}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=${WINDOW_AFTER}    omit=speed
    Check Response Status Code    200    ${omitted.status_code}
    Dictionary Should Contain Key    ${omitted.json()[0]}    brand
    Dictionary Should Not Contain Key    ${omitted.json()[0]}    speed

IOP_EXT_TMP_04_08 The Attribute List Selects Remote Entities Holding One Of Them
    [Documentation]    5.7.4.3: Attribute names "of which at least one shall
    ...    exist in order for an Entity to be selected, and also used as
    ...    query projection attributes" — a remote series without the listed
    ...    Attribute is dropped, one with it is kept and projected.
    [Tags]    iop    iop-ext    5_7_4    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${with}=    Evaluate
    ...    {"id": $entity_id + "-in", "type": $etype, "speed": [{"type": "Property", "value": 4, "observedAt": "2026-05-01T00:00:00Z"}], "brand": [{"type": "Property", "value": "Acme", "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${with}
    ${without}=    Evaluate
    ...    {"id": $entity_id + "-out", "type": $etype, "brand": [{"type": "Property", "value": "Zed", "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${without}

    ${response}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=${WINDOW_AFTER}    attrs=speed
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-in
    Should Not Contain    ${response.text}    ${entity_id}-out
    Should Not Contain    ${response.text}    Acme


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopTmd${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopTmd:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:ioptmd-${suffix}

Label Instances
    [Documentation]    5.2.20: a single instance may be rendered bare, a
    ...    series as an array — normalize to a list.
    [Arguments]    ${entity}
    ${label}=    Set Variable    ${entity}[label]
    ${instances}=    Evaluate    $label if isinstance($label, list) else [$label]
    RETURN    ${instances}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-3
    FOR    ${tail}    IN    ${EMPTY}    -b2    -b3    -in    -out
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b3_url}    ${entity_id}${tail}
        Delete Temporal Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Temporal Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Temporal Via Broker    ${b3_url}    ${entity_id}${tail}
    END
