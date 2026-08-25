*** Settings ***
Documentation       Verify 5.7.4.4 S1/S2 values-filter edges the official 021
...                 TPs skip: "the values filter query shall be checked
...                 against all the Attribute instances resulting from the
...                 initial filtering performed by the temporal query" — an
...                 instance OUTSIDE the window must never satisfy q, an
...                 instance inside must, whatever the value shape (numbers,
...                 strings, Relationship objects, arrays, datasetId
...                 instances, ranges, lists, regex/unequal/negated forms,
...                 offset timestamps).
...
...                 These cases deliberately exercise every branch of the
...                 store-side q prefilter (compilable leaf, And-with-refused
...                 member, Or-with-refused-branch, trivial shapes) so the
...                 pg/timescale CI cells prove SQL and the in-memory
...                 arbiter answer identically. Antares extension TP.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create Fixture Entities
Suite Teardown      Delete Fixture Entities


*** Variables ***
${fast}=        urn:ngsi-ld:Vehicle:qp5744-fast
${slow}=        urn:ngsi-ld:Vehicle:qp5744-slow
${shapes}=      urn:ngsi-ld:Vehicle:qp5744-shapes
${nowin}=       urn:ngsi-ld:Vehicle:qp5744-nowin
${offset}=      urn:ngsi-ld:Vehicle:qp5744-offset
${multi}=       urn:ngsi-ld:Vehicle:qp5744-multi
${IN}=          2026-03-01T12:00:00Z
${OUT}=         2026-01-05T00:00:00Z
${window}=      timerel=between&timeAt=2026-03-01T00:00:00Z&endTimeAt=2026-03-02T00:00:00Z


*** Test Cases ***
5744_01_01 Out-Of-Window Instance Never Satisfies Q
    [Documentation]    5.7.4.4 S2: slow's only speed>25 instance (80) is
    ...    outside the window — the entity must not be returned; fast's
    ...    in-window 30 qualifies it.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%3E25
    Should Contain    ${response.text}    ${fast}
    Should Not Contain    ${response.text}    ${slow}
    Should Not Contain    ${response.text}    ${nowin}

5744_01_02 Returned Instances Respect The Window
    [Documentation]    5.7.4.4 S1: the temporal representation carries only
    ...    in-window instances — the January stamps must not appear.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%3E25
    Should Contain    ${response.text}    ${IN}
    Should Not Contain    ${response.text}    ${OUT}

5744_01_03 Offset Timestamp Is Rejected On Creation
    [Documentation]    4.6.3: DateTime is the fixed-width UTC ("Z") form —
    ...    an observedAt carrying a +03:00 offset is BadRequestData, so the
    ...    byte-ordered window comparison always agrees with instant order.
    [Tags]    te-query    5_7_4    since_v1.9.1
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${temporal_api_url}/temporal/entities
    ...    data={"id": "${offset}", "type": "Vehicle", "speed": [{"type": "Property", "value": 50, "observedAt": "2026-03-01T02:00:00+03:00"}]}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Should Contain    ${response.text}    BadRequestData

5744_01_04 And Across Attributes
    [Documentation]    4.9: q=speed>=5;heading<90 — every conjunct must hold
    ...    on windowed instances; an entity missing the attribute fails it.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%3E%3D5%3Bheading%3C90
    Should Contain    ${response.text}    ${fast}
    Should Not Contain    ${response.text}    ${slow}
    Should Not Contain    ${response.text}    ${multi}

5744_01_05 Or Of Comparisons
    [Documentation]    4.9: q=speed>25|heading>100 — either windowed branch
    ...    qualifies the entity.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%3E25%7Cheading%3E100
    Should Contain    ${response.text}    ${fast}
    Should Contain    ${response.text}    ${slow}
    Should Not Contain    ${response.text}    ${shapes}

5744_01_06 Or With A Regex Branch
    [Documentation]    4.9: q=speed>25|name~="^m" — a pattern branch mixes
    ...    with a comparison branch without losing either side.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%3E25%7Cname~%3D%22%5Em%22
    Should Contain    ${response.text}    ${fast}
    Should Contain    ${response.text}    ${shapes}
    Should Not Contain    ${response.text}    ${nowin}

5744_01_07 Unequal Comparison
    [Documentation]    4.9: q=speed!=10 — an in-window unequal instance
    ...    qualifies; an entity with no windowed instances never does.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%21%3D10
    Should Contain    ${response.text}    ${fast}
    Should Not Contain    ${response.text}    ${nowin}

5744_01_08 Negated Existence In A Conjunction
    [Documentation]    4.9: q=!heading;speed>25 — absence of the attribute
    ...    (within the window) is a satisfiable condition.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    %21heading%3Bspeed%3E25
    Should Contain    ${response.text}    ${multi}
    Should Not Contain    ${response.text}    ${fast}

5744_01_09 Equality Against A Value Range
    [Documentation]    4.9: q=speed==10..40 is a closed interval over the
    ...    windowed instances.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%3D%3D10..40
    Should Contain    ${response.text}    ${fast}
    Should Contain    ${response.text}    ${slow}
    Should Not Contain    ${response.text}    ${nowin}
    Should Not Contain    ${response.text}    ${shapes}

5744_01_10 Equality Against A Value List
    [Documentation]    4.9: q=route=="550","551" matches any of the listed
    ...    values.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    route%3D%3D%22550%22,%22551%22
    Should Contain    ${response.text}    ${fast}
    Should Not Contain    ${response.text}    ${slow}
    Should Not Contain    ${response.text}    ${shapes}

5744_01_11 Relationship Object Comparison
    [Documentation]    4.9: q=ref=="urn:dest:qp1" compares the Relationship
    ...    object, not a Property value.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    ref%3D%3D%22urn:dest:qp1%22
    Should Contain    ${response.text}    ${shapes}
    Should Not Contain    ${response.text}    ${fast}

5744_01_12 Array Value Element Match
    [Documentation]    4.9: for an array Property value, equality matches
    ...    any element.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    tags%3D%3D%22a%22
    Should Contain    ${response.text}    ${shapes}
    Should Not Contain    ${response.text}    ${fast}

5744_01_13 DatasetId Instance Qualifies The Entity
    [Documentation]    4.5.5/5.7.4.4: an in-window instance carrying a
    ...    datasetId counts like any other for the values filter.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Temporal    speed%3E50
    Should Contain    ${response.text}    ${multi}
    Should Not Contain    ${response.text}    ${fast}
    Should Not Contain    ${response.text}    ${slow}


*** Keywords ***
Query Temporal
    [Arguments]    ${q_expr}
    ${response}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&q=${q_expr}&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    RETURN    ${response}

Create Temporal Entity
    [Arguments]    ${payload}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${temporal_api_url}/temporal/entities
    ...    data=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Create Fixture Entities
    Create Temporal Entity    {"id": "${fast}", "type": "Vehicle", "speed": [{"type": "Property", "value": 30, "observedAt": "${IN}"}, {"type": "Property", "value": 90, "observedAt": "${OUT}"}], "heading": [{"type": "Property", "value": 45, "observedAt": "${IN}"}], "route": [{"type": "Property", "value": "550", "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${slow}", "type": "Vehicle", "speed": [{"type": "Property", "value": 10, "observedAt": "${IN}"}, {"type": "Property", "value": 80, "observedAt": "${OUT}"}], "heading": [{"type": "Property", "value": 170, "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${shapes}", "type": "Vehicle", "name": [{"type": "Property", "value": "m", "observedAt": "${IN}"}], "ref": [{"type": "Relationship", "object": "urn:dest:qp1", "observedAt": "${IN}"}], "tags": [{"type": "Property", "value": ["a", "b"], "observedAt": "${IN}"}]}
    Create Temporal Entity    {"id": "${nowin}", "type": "Vehicle", "speed": [{"type": "Property", "value": 99, "observedAt": "${OUT}"}]}
    Create Temporal Entity    {"id": "${multi}", "type": "Vehicle", "speed": [{"type": "Property", "value": 5, "observedAt": "${IN}"}, {"type": "Property", "value": 60, "observedAt": "${IN}", "datasetId": "urn:ngsi-ld:Dataset:qp1"}]}

Delete Fixture Entities
    FOR    ${id}    IN    ${fast}    ${slow}    ${shapes}    ${nowin}    ${offset}    ${multi}
        ${response}=    DELETE    url=${temporal_api_url}/temporal/entities/${id}    expected_status=any
    END
