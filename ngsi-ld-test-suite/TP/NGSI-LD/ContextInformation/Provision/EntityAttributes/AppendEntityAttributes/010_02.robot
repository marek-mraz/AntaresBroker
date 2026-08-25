*** Settings ***
Documentation       Check that one cannot append entity attributes with invalid/missing id or invalid request body

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Append Attributes


*** Variables ***
${filename}=                        vehicle-speed-two-datasetid.jsonld
${fragment_filename}=               vehicle-attribute-to-add-fragment.jsonld
${status_code}=                     400
${invalid_fragment_filename}=       invalid-fragment.jsonld


*** Test Cases ***    ENTITY_INVALID_ID
010_02_01 Append Entity Attributes If The Entity Id Is Not Present
    ${EMPTY}
010_02_02 Append Entity Attributes If The Entity Id Is Not A Valid URI
    thisisaninvaliduri


*** Keywords ***
Append Attributes
    [Documentation]    Check that one cannot append entity attributes with invalid/missing id or invalid request body
    [Tags]    ea-append    5_6_3
    [Arguments]    ${entity_invalid_id}
    ${response}=    Append Entity Attributes    ${entity_invalid_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    [Teardown]    Delete Entity    ${entity_id}

Create Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    Set Test Variable    ${entity_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
