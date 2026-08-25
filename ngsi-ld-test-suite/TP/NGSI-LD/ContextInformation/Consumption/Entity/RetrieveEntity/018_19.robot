*** Settings ***
Documentation       Check that a ResourceNotFound error is returned if the entity has no members left after applying pick and omit query params

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Created Entity
Test Template       Retrieve Entity With Pick And Omit Query Params


*** Variables ***
${filename}=    building-simple-attributes.jsonld


*** Test Cases ***    PICK    OMIT
018_19_01 RetrieveWithUnmatchedPick
    [Documentation]    Check that a ResourceNotFound error is returned if pick does not match any entity member
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    unknown    ${EMPTY}
018_19_02 RetrieveWithFullExcludingOmit
    [Documentation]    Check that a ResourceNotFound error is returned if omit contains all entity members
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    ${EMPTY}    id,type,name,subCategory,airQualityLevel,almostFull


*** Keywords ***
Retrieve Entity With Pick And Omit Query Params
    [Documentation]    Check that a ResourceNotFound error is returned if the entity has no members left after applying pick and omit query params
    [Arguments]    ${pick}    ${omit}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    ...    pick=${pick}
    ...    omit=${omit}

    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Create Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}

Delete Created Entity
    Delete Entity    ${entity_id}
