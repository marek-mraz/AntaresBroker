*** Settings ***
Documentation       Check that one cannot create an entity with an existing id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entity
Suite Teardown      Delete Initial Entity


*** Variables ***
${filename}=        building-minimal.jsonld
${content_type}=    application/ld+json


*** Test Cases ***
001_03_01 Create One Valid Entity And One Invalid Entity
    [Documentation]    Check that one cannot create an entity with an existing id
    [Tags]    e-create    5_6_1
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${content_type}

    Check Response Status Code    409    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_ALREADY_EXISTS}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${content_type}
    Check Response Status Code    201    ${create_response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}
