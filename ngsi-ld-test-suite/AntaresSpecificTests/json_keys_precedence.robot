*** Settings ***
Documentation       Verify that an Attribute named in both `expandValues` and
...                 `jsonKeys` is left out of the JSON-LD type coercion.
...
...                 ANTARES DECISION, NOT A CIM 009 REQUIREMENT. Clause 4.9
...                 defines both lists — `expandValues` names the Attributes
...                 whose query-term values "should be expanded against the
...                 supplied @context using JSON-LD type coercion prior to
...                 executing the query", `jsonKeys` names those whose values
...                 "are to be considered uninterpretable as JSON-LD and should
...                 not be expanded" the same way — and states no precedence for
...                 an Attribute that appears in both. Both are SHOULD, so
...                 neither list overrides the other by its own wording.
...
...                 Antares subtracts: `jsonKeys` says what the value IS,
...                 `expandValues` only asks for a coercion before the
...                 comparison, and coercing a value the client has declared
...                 unreadable builds a term the stored value can never carry.
...                 The request is not refused — nothing in 4.9 forbids naming
...                 an Attribute twice — and an Attribute in only one list is
...                 unaffected.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create The Probe Entity
Suite Teardown      DELETE    url=${url}/entities/${probe_id}    expected_status=any


*** Variables ***
${probe_id}         urn:ngsi-ld:AntaresJsonKeysProbe:1
${probe_query}      type=AntaresJsonKeysProbe&q=category%3D%3Dcommercial


*** Test Cases ***
expandValues Alone Coerces The Query Term
    [Documentation]    4.9 EXAMPLE 12: without the coercion the literal does not
    ...    match the VocabProperty's expanded URI, with it it does. This is the
    ...    behaviour the decision below is measured against.
    [Tags]    antares-specific    query-language    expand-values
    Query Should Match    ${probe_query}&expandValues=category
    Query Should Not Match    ${probe_query}

An Attribute In Both Lists Is Not Expanded
    [Documentation]    `jsonKeys` takes the Attribute back out of the expansion,
    ...    so the query behaves as if `expandValues` had not named it.
    [Tags]    antares-specific    query-language    json-keys
    Query Should Not Match    ${probe_query}&expandValues=category&jsonKeys=category

jsonKeys Only Removes The Attributes It Names
    [Documentation]    A `jsonKeys` entry for another Attribute leaves the
    ...    expansion alone, so the subtraction cannot be read as "any jsonKeys
    ...    disables expandValues".
    [Tags]    antares-specific    query-language    json-keys
    Query Should Match    ${probe_query}&expandValues=category&jsonKeys=colour


*** Keywords ***
Create The Probe Entity
    [Documentation]    A VocabProperty in the core @context: 4.5.22 expands its
    ...    `vocab` term the way `expandValues` expands the query term, which is
    ...    what makes the two comparable at all.
    ${payload}=    Evaluate
    ...    {"id": "${probe_id}", "type": "AntaresJsonKeysProbe", "category": {"type": "VocabProperty", "vocab": "commercial"}}
    ${headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${url}/entities    json=${payload}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Query Should Match
    [Arguments]    ${query}
    ${response}=    GET    url=${url}/entities?${query}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    [e["id"] for e in $response.json()]
    Should Contain    ${ids}    ${probe_id}

Query Should Not Match
    [Arguments]    ${query}
    ${response}=    GET    url=${url}/entities?${query}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    [e["id"] for e in $response.json()]
    Should Not Contain    ${ids}    ${probe_id}
