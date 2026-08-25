*** Settings ***
Documentation       Check that one cannot get an entity if the entity id is not known to the system

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
018_03_01 Get An Entity If The Entity Id Is Not Known To The System
    [Documentation]    Check that one cannot get an entity if the entity id is not known to the system
    [Tags]    e-retrieve    5_7_1
    ${entity_id}=    Generate Random Building Entity Id
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
