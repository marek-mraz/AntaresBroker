*** Settings ***
Documentation       Check that one cannot append entity attributes with invalid/missing id or invalid request body

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity


*** Variables ***
${filename}=                        vehicle-speed-two-datasetid.jsonld
${fragment_filename}=               vehicle-attribute-to-add-fragment.jsonld
${status_code}=                     400
${invalid_fragment_filename}=       invalid-fragment.jsonld


*** Test Cases ***
010_05_01 Append Entity Attributes With Invalid Entity Fragments
    [Documentation]    Check that one cannot append entity attributes with invalid entity fragments
    [Tags]    ea-append    5_6_3
    ${response}=    Append Entity Attributes
    ...    ${entity_id}
    ...    ${invalid_fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
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
