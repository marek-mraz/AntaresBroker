*** Settings ***
Documentation       Verify the 4.5.3.2 normalized Relationship member prohibitions.
...
...                 Clause 4.5.3.2: a normalized NGSI-LD Relationship "shall never
...                 include" unitCode ("as Relationships are unitless"), the
...                 Property-family value members (value, languageMap, json, vocab,
...                 valueList), objectList, or the output-only previous* members.
...                 objectType (4.5.23) stays a legal optional member.
...
...                 Antares extension TP — no official TP asserts the Prohibited
...                 list of clause 4.5.3.2.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Entity With Relationship Carrying


*** Test Cases ***    MEMBER JSON    EXPECTED STATUS
453_01_01 Relationship With unitCode
    [Tags]    e-create    4_5_3_2    since_v1.9.1
    {"unitCode": "MTR"}    400
453_01_02 Relationship With A Property Value
    [Tags]    e-create    4_5_3_2    since_v1.9.1
    {"value": 42}    400
453_01_03 Relationship With A LanguageMap
    [Tags]    e-create    4_5_3_2    since_v1.9.1
    {"languageMap": {"en": "hello"}}    400
453_01_04 Relationship With An Output-Only previousObject
    [Tags]    e-create    4_5_3_2    since_v1.9.1
    {"previousObject": "urn:ngsi-ld:Other:0"}    400
453_01_05 Relationship With A Legal objectType
    [Tags]    e-create    4_5_3_2    since_v1.9.1
    {"objectType": "OffStreetParking"}    201


*** Keywords ***
Create Entity With Relationship Carrying
    [Documentation]    4.5.3.2: prohibited members on a normalized Relationship are
    ...    invalid content (400 BadRequestData); objectType is optional and legal.
    [Arguments]    ${member}    ${expected}
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${extra}=    Evaluate    json.loads('''${member}''')    json
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "isParked": {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:1", **$extra}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    ${expected}    ${response.status_code}
    IF    ${expected} == 400
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    ${response.json()}
        ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ELSE
        Delete Entity    ${entity_id}
    END
