*** Settings ***
Documentation       Verify the 4.5.3.2 normalized Relationship member prohibitions.
...
...                 Clause 4.5.3.2: a normalized NGSI-LD Relationship "shall never
...                 include" unitCode ("as Relationships are unitless"), the
...                 Property-family value members (value, languageMap, json, vocab,
...                 valueList), objectList, the output-only previous* members, or
...                 entityIdSealed/entityTypeSealed. The sealed members carry no
...                 exception here: 4.5.2.2 grants one only "unless the Property
...                 name is ngsildproof", so a Relationship named ngsildproof is
...                 prohibited from carrying them just the same.
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
453_01_06 Relationship With A Sealed Entity Id
    [Tags]    e-create    4_5_3_2    since_v1.9.1
    {"entityIdSealed": "urn:ngsi-ld:OffStreetParking:1"}    400
453_01_07 Relationship With A Sealed Entity Type
    [Tags]    e-create    4_5_3_2    since_v1.9.1
    {"entityTypeSealed": "OffStreetParking"}    400
453_01_08 Relationship Named ngsildproof With A Sealed Entity Id
    [Tags]    e-create    4_5_3_2    since_v1.9.1
    [Template]    Create Entity With Named Relationship Carrying
    ngsildproof    {"entityIdSealed": "urn:ngsi-ld:OffStreetParking:1"}    400
453_01_09 Relationship Named ngsildproof With A Sealed Entity Type
    [Tags]    e-create    4_5_3_2    since_v1.9.1
    [Template]    Create Entity With Named Relationship Carrying
    ngsildproof    {"entityTypeSealed": "OffStreetParking"}    400


*** Keywords ***
Create Entity With Relationship Carrying
    [Documentation]    4.5.3.2: prohibited members on a normalized Relationship are
    ...    invalid content (400 BadRequestData); objectType is optional and legal.
    [Arguments]    ${member}    ${expected}
    Create Entity With Named Relationship Carrying    isParked    ${member}    ${expected}

Create Entity With Named Relationship Carrying
    [Documentation]    The same prohibitions under a chosen Relationship name.
    ...    4.5.2.2 excepts entityIdSealed/entityTypeSealed only for the
    ...    ngsildproof PROPERTY, so the name alone does not lift the 4.5.3.2 ban.
    [Arguments]    ${attr_name}    ${member}    ${expected}
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${extra}=    Evaluate    json.loads('''${member}''')    json
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], $attr_name: {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:1", **$extra}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    ${expected}    ${response.status_code}
    IF    ${expected} == 400
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    ${response.json()}
        ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ELSE
        Delete Entity    ${entity_id}
    END
