*** Settings ***
Documentation       Check that one cannot append entity attributes if the entity id is not known to the system

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Teardown      Delete Initial Entities


*** Variables ***
${fragment_filename}=       vehicle-attribute-to-add-fragment.jsonld


*** Test Cases ***
010_03_01 Append Entity Attributes When The Entity Id Is Not Known To The System
    [Documentation]    Check that one cannot append entity attributes if the entity id is not known to the system
    [Tags]    ea-append    5_6_3
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Append Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
Delete Initial Entities
    Delete Entity    ${entity_id}
