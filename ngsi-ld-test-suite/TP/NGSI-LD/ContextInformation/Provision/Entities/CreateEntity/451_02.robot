*** Settings ***
Documentation       Verify the 4.5.1 Entity id value space.
...
...                 Clause 4.5.1: '"id" whose value shall be a URI that identifies
...                 the Entity.' RFC 3986 admits no whitespace in a URI and excludes
...                 the ASCII characters used for markup and quoting. RFC 3987 widens
...                 the repertoire to the non-ASCII characters of an IRI, but its
...                 ucschar production still contains the invisible and bidi-control
...                 characters — an id carrying one of those renders as a different
...                 id in logs, UIs and audit trails, so it identifies no Entity.
...
...                 Antares extension TP — the official CreateEntity set never posts
...                 a malformed id, so this negative surface is uncovered.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Entity Whose Id Carries Codepoint


*** Test Cases ***    CODEPOINT
451_02_01 Right-To-Left Override In Entity Id
    [Tags]    e-create    4_5_1
    202E
451_02_02 Zero Width Space In Entity Id
    [Tags]    e-create    4_5_1
    200B
451_02_03 Byte Order Mark In Entity Id
    [Tags]    e-create    4_5_1
    FEFF
451_02_04 Line Separator In Entity Id
    [Tags]    e-create    4_5_1
    2028
451_02_05 No-Break Space In Entity Id
    [Tags]    e-create    4_5_1
    00A0
451_02_06 Markup Delimiter In Entity Id
    [Tags]    e-create    4_5_1
    003C
451_02_08 Variation Selector In Entity Id
    [Tags]    e-create    4_5_1
    FE0F
451_02_09 Supplementary Variation Selector In Entity Id
    [Tags]    e-create    4_5_1
    E0100
451_02_10 Noncharacter In Entity Id
    [Tags]    e-create    4_5_1
    FDD0
451_02_11 Plane-Trailing Noncharacter In Entity Id
    [Tags]    e-create    4_5_1
    FFFE
451_02_12 Hangul Choseong Filler In Entity Id
    [Tags]    e-create    4_5_1
    115F
451_02_13 Hangul Filler In Entity Id
    [Tags]    e-create    4_5_1
    3164
451_02_14 Halfwidth Hangul Filler In Entity Id
    [Tags]    e-create    4_5_1
    FFA0
451_02_15 Braille Pattern Blank In Entity Id
    [Tags]    e-create    4_5_1
    2800

451_02_07 An IRI Relationship Object Is Still Accepted
    [Documentation]    The prohibition covers the invisible characters only: the
    ...    ordinary non-ASCII characters RFC 3987 admits in an IRI shall keep
    ...    identifying an Entity, so this create shall NOT be rejected.
    [Tags]    e-create    4_5_1
    [Template]    NONE
    [Teardown]    Delete Entity    ${entity_id}
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${object_id}=    Evaluate    "urn:ngsi-ld:Ciudad:Par" + chr(0xED) + "s"
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "isParked": {"type": "Relationship", "object": $object_id}, "@context": [$ngsild_test_suite_context]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}


*** Keywords ***
Create Entity Whose Id Carries Codepoint
    [Documentation]    4.5.1: an id that is not a URI shall be rejected with
    ...    BadRequestData, so the Entity is never created under an id nobody
    ...    can read back.
    [Arguments]    ${codepoint}
    ${entity_id}=    Evaluate    "urn:ngsi-ld:Vehicle:A" + chr(0x${codepoint}) + "B"
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
