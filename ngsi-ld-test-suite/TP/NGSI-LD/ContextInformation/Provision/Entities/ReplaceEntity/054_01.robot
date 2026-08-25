*** Settings ***
Documentation       Check that one can replace an existing entity and that its createdAt Temporal Property remains unchanged

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity


*** Variables ***
${entity_filename}                  building-simple-attributes.json
${entity_replacement_filename}      building-locatedAt-and-name.json
${expectation_filename}             building-locatedAt-and-name-normalized.jsonld


*** Test Cases ***
054_01_01 Replace An Existing Entity
    [Documentation]    Check that one can replace an existing entity and that its createdAt Temporal Property remains unchanged
    [Tags]    e-replace    5_6_18    6_5_3_3    since_v1.6.1
    ${entity}=    Load Entity
    ...    entity_file_name=${entity_replacement_filename}
    ...    entity_id=${entity_id}
    ${response}=    Replace Entity Selecting Content Type
    ...    entity_id=${entity_id}
    ...    entity_fragment=${entity}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    options=sysAttrs
    ${ignored_attributes}=    Create List    @context    createdAt    modifiedAt
    ${entity_expectation_payload}=    Load Test Sample
    ...    test_sample_file_path=entities/expectations/${expectation_filename}
    ...    test_sample_id=${entity_id}
    Check Updated Resource Set To
    ...    updated_resource=${entity_expectation_payload}
    ...    response_body=${response1.json()}
    ...    ignored_keys=${ignored_attributes}
    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=createdAt
    ...    response_body=${response1.json()}
    ...    expected_attribute_value=${createdAt}


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    options=sysAttrs
    ${createdAt}=    Set Variable    ${response.json()['createdAt']}
    Set Test Variable    ${createdAt}

Delete Initial Entity
    Delete Entity    ${entity_id}
