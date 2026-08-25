*** Settings ***
Documentation       Check that one cannot update entity attributes with invalid request body

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Initialize Environment    ${filename}
Test Template       Update entity attributes with invalid entity fragments


*** Variables ***
${filename}=    vehicle-speed-two-datasetid.jsonld


*** Test Cases ***
011_04_01 Update Entity Attributes With Invalid Entity Fragments
    vehicle-speed-two-datasetid.jsonld    invalid-fragment.jsonld


*** Keywords ***
Update entity attributes with invalid entity fragments
    [Documentation]    Check that one cannot update an attribute if the entity fragment is invalid
    [Tags]    ea-update    5_6_2
    [Arguments]    ${filename}    ${fragment_filename}
    ${response}=    Update Entity Attributes
    ...    ${entity_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    [Teardown]    Delete Entity    ${entity_id}

Initialize Environment
    [Arguments]    ${filename}
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
