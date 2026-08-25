*** Settings ***
Documentation       Check the NGSI-LD Scope grammar (CIM 009 clause 4.18) on entity
...                 creation: Scope = [/] ScopeLevel *(/ScopeLevel) with ScopeLevel =
...                 unicodeLetter *(letter/digit/_), and "urn:ngsi-ld:null" shall be
...                 "only used and only appear in case of deleted scopes" — never on
...                 create. The official scope TPs cover only well-formed scopes.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Entity With Scope Expecting


*** Test Cases ***    SCOPE_JSON    EXPECTED_STATUS
418_01_01 A Level Starting With A Digit Is Rejected
    [Documentation]    4.18: ScopeLevel = unicodeLetter *ScopeLevelChar — the first
    ...    character must be a letter
    [Tags]    e-create    4_18    since_v1.9.1
    "9bad"    400

418_01_02 An Empty Level Is Rejected
    [Documentation]    4.18 grammar admits no empty ScopeLevel between separators
    [Tags]    e-create    4_18    since_v1.9.1
    "/a//b"    400

418_01_03 A Dash Is Not A ScopeLevelChar
    [Documentation]    4.18: ScopeLevelChar is letter/digit/_ only
    [Tags]    e-create    4_18    since_v1.9.1
    "a-b"    400

418_01_04 The NGSI-LD Null Scope Cannot Be Created
    [Documentation]    4.18: "urn:ngsi-ld:null ... shall be only used and only appear
    ...    in case of deleted scopes"
    [Tags]    e-create    4_18    since_v1.9.1
    "urn:ngsi-ld:null"    400

418_01_05 One Malformed Entry Poisons A Scope Array
    [Documentation]    4.18: every entry of a multi-scope array must satisfy the grammar
    [Tags]    e-create    4_18    since_v1.9.1
    \["/ok", "9bad"]    400

418_01_06 The Clause Examples Are Accepted
    [Documentation]    4.18 EXAMPLES 1-4 (positive control)
    [Tags]    e-create    4_18    since_v1.9.1
    "/Madrid/Gardens/ParqueNorte"    201


*** Keywords ***
Create Entity With Scope Expecting
    [Arguments]    ${scope_json}    ${expected_status_code}
    ${entity_id}=    Generate Random Building Entity Id
    ${payload}=    Evaluate
    ...    json.dumps({"id": "${entity_id}", "type": "Building", "scope": json.loads('''${scope_json}''')})
    ...    json
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    data=${payload}
    ...    expected_status=any
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    ${expected_status_code} == 201    Delete Entity    ${entity_id}
