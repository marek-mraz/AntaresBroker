*** Settings ***
Documentation       Verify the 4.4 Core @context rules observable on the API.
...
...                 Clause 4.4: the core context "defines the term id, which is mapped
...                 to @id, and the term type, which is mapped to @type. Since @id and
...                 @type are what is typically used in JSON-LD, they may also be used
...                 in NGSI-LD requests [...]. In NGSI-LD responses, only id and type
...                 shall be used." And: "the Core @context is protected and shall
...                 remain immutable and invariant during expansion or compaction of
...                 terms [...] implementations shall consider the Core @context as if
...                 it were in the last position of the @context array."
...
...                 Antares extension TP — no official TP is tagged 4_4.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
404_01_01 JSON-LD Keywords Accepted On Input And Aliased On Output
    [Documentation]    4.4: @id and @type may be used in requests wherever
    ...    applicable; in responses only id and type shall be used.
    [Tags]    e-create    e-retrieve    4_4    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"@id": $entity_id, "@type": "Vehicle", "speed": {"type": "Property", "value": 42}, "@context": [$ngsild_test_suite_context]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    Dictionary Should Contain Key    ${body}    id
    Dictionary Should Contain Key    ${body}    type
    Dictionary Should Not Contain Key    ${body}    @id
    Dictionary Should Not Contain Key    ${body}    @type

404_01_02 A User Context Cannot Redefine A Core Term
    [Documentation]    4.4: the Core @context is protected — processed as if in the
    ...    last position — so a user context redefining the core term Property does
    ...    not change how the attribute expands, and the entity round-trips intact.
    [Tags]    e-create    e-retrieve    4_4    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "speed": {"type": "Property", "value": 42}, "@context": [{"Property": "https://evil.example/Property"}, $ngsild_test_suite_context]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${attr_type}=    Evaluate    $response.json()['speed']['type']
    Should Be Equal    ${attr_type}    Property
