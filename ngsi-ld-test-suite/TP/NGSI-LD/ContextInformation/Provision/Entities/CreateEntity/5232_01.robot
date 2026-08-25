*** Settings ***
Documentation       Check the LanguageProperty data type (CIM 009 clause 5.2.32,
...                 Table 5.2.32-1): languageMap keys are non-empty language tags mapping
...                 to strings or arrays of strings, and valueType, when present, "shall
...                 be equal to langString".
...
...                 Antares extension TP — the official LP TPs never send valueType or a
...                 malformed languageMap.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Entity With LanguageProperty Expecting


*** Test Cases ***    EXPECTED_STATUS    ATTR_JSON
5232_01_01 A Conformant LanguageProperty With ValueType LangString Is Created
    [Documentation]    control: Table 5.2.32-1-conformant LP stays creatable
    [Tags]    e-create    5_2_32    since_v1.9.1
    201    {"type": "LanguageProperty", "languageMap": {"en": "hello", "sk": ["ahoj", "servus"]}, "valueType": "langString"}

5232_01_02 A ValueType Other Than LangString Is Rejected
    [Documentation]    Table 5.2.32-1: valueType "shall be equal to langString"
    [Tags]    e-create    5_2_32    since_v1.9.1
    400    {"type": "LanguageProperty", "languageMap": {"en": "x"}, "valueType": "xsd:string"}

5232_01_03 A Non-String LanguageMap Value Is Rejected
    [Documentation]    Table 5.2.32-1: values shall be JSON strings or arrays of strings
    [Tags]    e-create    5_2_32    since_v1.9.1
    400    {"type": "LanguageProperty", "languageMap": {"en": 5}}

5232_01_04 A Mixed-Type Array Value Is Rejected
    [Tags]    e-create    5_2_32    since_v1.9.1
    400    {"type": "LanguageProperty", "languageMap": {"en": ["a", 5]}}

5232_01_05 An Empty Language Tag Is Rejected
    [Documentation]    4.5.18.2: language tags are non-empty
    [Tags]    e-create    5_2_32    since_v1.9.1
    400    {"type": "LanguageProperty", "languageMap": {"": "x"}}

5232_01_06 A LanguageProperty Without LanguageMap Is Rejected
    [Documentation]    Table 5.2.32-1: languageMap is the mandatory value member
    [Tags]    e-create    5_2_32    since_v1.9.1
    400    {"type": "LanguageProperty"}


*** Keywords ***
Create Entity With LanguageProperty Expecting
    [Arguments]    ${expected_status_code}    ${attr_json}
    ${entity_id}=    Generate Random Building Entity Id
    ${payload}=    Evaluate
    ...    json.dumps({"id": "${entity_id}", "type": "Building", "probe": json.loads('''${attr_json}''')})
    ...    json
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    data=${payload}
    ...    expected_status=any
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    ${expected_status_code} == 400
        ${err_type}=    Evaluate    $response.json().get('type', '')
        Should Be Equal    ${err_type}    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ELSE
        Delete Entity    ${entity_id}
    END
