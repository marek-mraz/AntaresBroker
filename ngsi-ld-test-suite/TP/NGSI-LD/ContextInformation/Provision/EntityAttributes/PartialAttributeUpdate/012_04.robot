*** Settings ***
Documentation       Check that one can perform a partial update on a LanguageProperty property

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Initiate Test Case
Test Teardown       Delete Initial Entities
Test Template       Update Attributes


*** Variables ***
${filename}=        building-language-property-sub-property.jsonld
${status_code}=     204


*** Test Cases ***    FRAGMENT_FILENAME    ATTRIBUTE_ID    EXPECTATION_FILENAME
012_04_01 Check That One Can Partially Update A LanguageProperty Property
    [Tags]    ea-partial-update    5_6_4    4_5_18    since_v1.4.1
    building-language-property-fragment.jsonld    street    building-language-property-update.jsonld


*** Keywords ***
Update Attributes
    [Documentation]    Check that one can perform a partial update on a LanguageProperty property
    [Arguments]    ${fragment_filename}    ${attribute_id}    ${expectation_filename}
    ${response}=    Partial Update Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=${attribute_id}
    ...    fragment_filename=${fragment_filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${ignored_attributes}=    Create List    @context
    Check Updated Resource Set To    ${entity_expectation_payload}    ${response1.json()}    ${ignored_attributes}

Initiate Test Case
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entities
    Delete Entity    ${entity_id}
