*** Settings ***
Documentation       Verify the 4.5.2.2 normalized Property member prohibitions.
...
...                 Clause 4.5.2.2: a normalized NGSI-LD Property "shall never
...                 include" the value-defining members of other attribute types
...                 (object, languageMap, json, vocab, valueList, objectList), the
...                 inline-retrieval members (entity, entityList), the showChanges
...                 output members (previousValue, ...), or entityIdSealed /
...                 entityTypeSealed outside ngsildproof. valueType is a legal
...                 optional member coerced into a datatype URI.
...
...                 Antares extension TP — no official TP asserts the Prohibited
...                 list of Table-less clause 4.5.2.2.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Entity With Property Carrying


*** Test Cases ***    MEMBER JSON    EXPECTED STATUS
452_01_01 Property With A Relationship Object
    [Tags]    e-create    4_5_2_2    since_v1.9.1
    {"object": "urn:ngsi-ld:Other:1"}    400
452_01_02 Property With A LanguageMap
    [Tags]    e-create    4_5_2_2    since_v1.9.1
    {"languageMap": {"en": "hello"}}    400
452_01_03 Property With A ValueList
    [Tags]    e-create    4_5_2_2    since_v1.9.1
    {"valueList": [1, 2]}    400
452_01_04 Property With An Output-Only previousValue
    [Tags]    e-create    4_5_2_2    since_v1.9.1
    {"previousValue": 0}    400
452_01_05 Property With entityIdSealed Outside ngsildproof
    [Tags]    e-create    4_5_2_2    since_v1.9.1
    {"entityIdSealed": true}    400
452_01_06 Property With A Legal valueType
    [Tags]    e-create    4_5_2_2    since_v1.9.1
    {"valueType": "xsd:double"}    201


*** Keywords ***
Create Entity With Property Carrying
    [Documentation]    4.5.2.2: prohibited members on a normalized Property are
    ...    invalid content (400 BadRequestData); valueType is optional and legal.
    [Arguments]    ${member}    ${expected}
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${extra}=    Evaluate    json.loads('''${member}''')    json
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 1, **$extra}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    ${expected}    ${response.status_code}
    IF    ${expected} == 400
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    ${response.json()}
        ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ELSE
        Delete Entity    ${entity_id}
    END
