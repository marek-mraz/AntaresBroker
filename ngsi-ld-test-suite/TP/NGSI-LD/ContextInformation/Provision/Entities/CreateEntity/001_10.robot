*** Settings ***
Documentation       Check that one can create an entity with a LanguageProperty property

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entity
Test Template       Create Entity Scenarios


*** Test Cases ***    FILENAME    CONTENT_TYPE
001_10_01 EntityWithLanguageProperty
    [Tags]    e-create    5_6_1    4_5_18    since_v1.4.1
    building-language-property.jsonld    application/ld+json


*** Keywords ***
Create Entity Scenarios
    [Documentation]    Check that one can create an entity with a LanguageProperty property
    [Arguments]    ${filename}    ${content_type}
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}

    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${content_type}

    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Headers Containing URI set to    ${entity_id}    ${response.headers}
    ${created_entity}=    Load Test Sample    entities/${filename}    ${entity_id}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${content_type}
    ...    context=${ngsild_test_suite_context}
    Check Created Resource Set To
    ...    created_resource=${created_entity}
    ...    response_body=${response1.json()}

Delete Initial Entity
    Delete Entity    ${entity_id}
