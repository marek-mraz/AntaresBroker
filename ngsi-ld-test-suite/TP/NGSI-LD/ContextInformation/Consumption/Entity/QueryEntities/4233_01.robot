*** Settings ***
Documentation       Verify 4.23.3 EXAMPLES 6/7: the collation parameter —
...                 orderBy string comparison under an ICU collation
...                 (IETF RFC 6067 tag) instead of codepoint order; an
...                 invalid tag is 400 BadRequestData.
...                 Antares extension TP — no official collation coverage.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Create Fixture Entities
Test Teardown       Delete Fixture Entities


*** Variables ***
${v_upper}=     urn:ngsi-ld:Vehicle:coll4233A
${v_b}=         urn:ngsi-ld:Vehicle:coll4233b
${v_accent}=    urn:ngsi-ld:Vehicle:coll4233acc


*** Test Cases ***
4233_01_01 Root Collation Ranks Accents With Their Base Letter
    [Documentation]    4.23.3 EXAMPLE 6/7: collation=und ranks "á" with "a"
    ...    (before "b"); the codepoint default ranks it after "b" — both
    ...    asserted so the collation path is proven distinct.
    [Tags]    em-usage    4_23_3    4_23_1    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/entities
    ...    params=type=Vehicle&idPattern=coll4233&orderBy=name
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${names}=    Evaluate    [e["name"]["value"] for e in $response.json()]
    Should Be Equal As Strings    ${names}    ['A', 'b', 'á']

    ${response}=    GET
    ...    url=${url}/entities
    ...    params=type=Vehicle&idPattern=coll4233&orderBy=name&collation=und
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${names}=    Evaluate    [e["name"]["value"] for e in $response.json()]
    Should Be Equal As Strings    ${names}    ['A', 'á', 'b']

4233_01_02 Invalid Collation Tag Is 400
    [Documentation]    An unparseable collation is BadRequestData, never a
    ...    silent codepoint fallback.
    [Tags]    em-usage    4_23_3    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/entities
    ...    params=type=Vehicle&orderBy=name&collation=!!nope
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Should Contain    ${response.text}    BadRequestData


*** Keywords ***
Create Fixture Entities
    Create Named Vehicle    ${v_upper}    A
    Create Named Vehicle    ${v_b}    b
    Create Named Vehicle    ${v_accent}    á

Create Named Vehicle
    [Arguments]    ${eid}    ${name}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${payload}=    Set Variable
    ...    {"id": "${eid}", "type": "Vehicle", "name": {"type": "Property", "value": "${name}"}}
    ${response}=    POST    url=${url}/entities    data=${payload.encode('utf-8')}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Delete Fixture Entities
    FOR    ${eid}    IN    ${v_upper}    ${v_b}    ${v_accent}
        ${response}=    DELETE    url=${url}/entities/${eid}    expected_status=any
    END
