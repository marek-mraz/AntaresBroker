*** Settings ***
Documentation       Check that one cannot update entity attributes if the entity id or attributes are not known to the system

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Teardown      Delete Initial Entities


*** Variables ***
${fragment_filename}=       vehicle-speed-two-datasetid-01-fragment.jsonld


*** Test Cases ***
011_03_01 Update Entity Attributes When The Entity Id Is Not Known To The System
    [Documentation]    Check that one cannot update entity attributes if the entity id or attributes are not known to the system
    [Tags]    ea-update    5_6_2
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
Delete Initial Entities
    Delete Entity    ${entity_id}
